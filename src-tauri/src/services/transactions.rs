use crate::domain::{
    protection::{is_protected_deb, is_protected_snap},
    AppError, ApplicationInstallation, DebDependencyPreview, HistoryEntry, Operation,
    OperationStatus, PackageFormat, UninstallPlan, UninstallPlanOptions,
};
use crate::services::privileged::{
    build_deb_pkexec_args, build_flatpak_system_pkexec_args, build_snap_pkexec_args,
    flatpak_system_scope_flag, format_bytes_fr, query_deb_status, run_pkexec, simulate_deb_removal,
    validate_deb_package_name, validate_flatpak_installation, validate_flatpak_ref,
    validate_snap_name, verify_deb_removed, verify_flatpak_removed, verify_snap_removed,
    FLATPAK_BIN, SNAP_BIN,
};
use crate::storage::{
    append_history, list_in_flight_operations, record_in_flight_operation,
    remove_in_flight_operation, InFlightOperation,
};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct TransactionManager {
    plans: Mutex<HashMap<String, UninstallPlan>>,
    current_operation: Mutex<Option<Operation>>,
}

static INSTANCE: OnceLock<TransactionManager> = OnceLock::new();
static PLAN_COUNTER: AtomicU64 = AtomicU64::new(1);
static OP_COUNTER: AtomicU64 = AtomicU64::new(1);

pub fn manager() -> &'static TransactionManager {
    INSTANCE.get_or_init(|| TransactionManager {
        plans: Mutex::new(HashMap::new()),
        current_operation: Mutex::new(None),
    })
}

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(target_os = "linux")]
fn linux_dev_major_minor(dev: u64) -> (u64, u64) {
    let major = ((dev >> 8) & 0xfff) | ((dev >> 32) & !0xfff);
    let minor = (dev & 0xff) | ((dev >> 12) & !0xff);
    (major, minor)
}

pub(crate) fn proc_locks_has_write_lock(locks_content: &str, lock_ids: &[String]) -> bool {
    if lock_ids.is_empty() {
        return false;
    }
    for line in locks_content.lines() {
        let tokens: Vec<&str> = line.split_whitespace().collect();
        if tokens.len() < 6 || tokens.get(1) == Some(&"->") {
            continue;
        }
        if tokens[3] == "WRITE" && lock_ids.iter().any(|id| id == tokens[5]) {
            return true;
        }
    }
    false
}

/// Checks whether APT/dpkg actually holds a kernel write lock on `/var/lib/dpkg/lock-frontend`
/// or `/var/lib/dpkg/lock` by matching their `(major:minor:inode)` against `/proc/locks`.
/// Avoids false positives from idle daemons like `unattended-upgrade-shutdown` or `packagekitd`.
pub fn is_dpkg_locked() -> bool {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::MetadataExt;

        let lock_paths = ["/var/lib/dpkg/lock-frontend", "/var/lib/dpkg/lock"];
        let mut lock_ids = Vec::new();
        for lock_path in &lock_paths {
            if let Ok(meta) = fs::metadata(lock_path) {
                let (major, minor) = linux_dev_major_minor(meta.dev());
                lock_ids.push(format!("{major:02x}:{minor:02x}:{}", meta.ino()));
            }
        }
        if lock_ids.is_empty() {
            return false;
        }
        if let Ok(locks_content) = fs::read_to_string("/proc/locks") {
            return proc_locks_has_write_lock(&locks_content, &lock_ids);
        }
        false
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

impl TransactionManager {
    pub fn prepare(
        &self,
        installation: &ApplicationInstallation,
        options: UninstallPlanOptions,
    ) -> Result<UninstallPlan, AppError> {
        let op_guard = self.current_operation.lock().unwrap();
        if let Some(ref op) = *op_guard {
            if matches!(
                op.status,
                OperationStatus::Running | OperationStatus::Verifying
            ) {
                return Err(AppError::new(
                    "TRANSACTION_IN_PROGRESS",
                    "Une opération de désinstallation est déjà en cours d’exécution.",
                ));
            }
        }
        drop(op_guard);

        // Validation against flag injection and malformed identifiers
        let disallow_whitespace = installation.format != PackageFormat::Local;
        if installation.native_id.starts_with('-')
            || installation
                .native_id
                .contains(|c: char| c == '\0' || (disallow_whitespace && c.is_whitespace()))
        {
            return Err(AppError::new(
                "INVALID_IDENTIFIER",
                "L’identifiant de l’application contient des caractères non autorisés.",
            ));
        }

        if let Some(ref r) = installation.reference {
            if r.starts_with('-')
                || r.contains(|c: char| c == '\0' || (disallow_whitespace && c.is_whitespace()))
            {
                return Err(AppError::new(
                    "INVALID_IDENTIFIER",
                    "La référence de l’application contient des caractères non autorisés.",
                ));
            }
        }

        let now = current_timestamp();
        let counter = PLAN_COUNTER.fetch_add(1, Ordering::Relaxed);
        let plan_id = format!(
            "plan_{}_{}_{}_{}",
            match installation.format {
                PackageFormat::Deb => "deb",
                PackageFormat::Snap => "snap",
                PackageFormat::Flatpak => "flatpak",
                PackageFormat::Appimage => "appimage",
                PackageFormat::Local => "local",
            },
            installation.native_id.replace(['/', ':', ' '], "_"),
            now,
            counter
        );

        let mut deb_preview: Option<DebDependencyPreview> = None;
        let (requires_auth, affected_items, data_impact, is_blocked, blocked_reason) =
            match installation.format {
                PackageFormat::Deb => {
                    validate_deb_package_name(&installation.native_id)?;
                    if is_protected_deb(&installation.native_id) {
                        (
                            true,
                            vec![format!("Paquet DEB : {}", installation.native_id)],
                            "Ce paquet est protégé et ne sera pas modifié.".into(),
                            true,
                            Some(
                                "Ce paquet est essentiel au fonctionnement du système Ubuntu et ne peut pas être supprimé."
                                    .into(),
                            ),
                        )
                    } else if is_dpkg_locked() {
                        (
                            true,
                            vec![format!("Paquet DEB : {}", installation.native_id)],
                            "Une autre opération APT/dpkg est en cours.".into(),
                            true,
                            Some(
                                "Le gestionnaire de paquets APT/dpkg est actuellement verrouillé par une autre opération système. Veuillez réessayer ultérieurement."
                                    .into(),
                            ),
                        )
                    } else {
                        match simulate_deb_removal(&installation.native_id, options.purge_config) {
                            Ok(sim) if !sim.is_installed => (
                                true,
                                vec![format!("Paquet DEB : {}", installation.native_id)],
                                "Ce paquet n’est plus installé sur le système.".into(),
                                true,
                                Some(
                                    "Le paquet DEB ciblé n’est plus installé sur le système. Actualisez l’inventaire."
                                        .into(),
                                ),
                            ),
                            Ok(sim) => {
                                let has_blockers = !sim.preview.blocking_dependents.is_empty();
                                let mut items: Vec<String> = sim
                                    .preview
                                    .removed_packages
                                    .iter()
                                    .map(|pkg| {
                                        if pkg.is_target {
                                            format!("Paquet DEB : {}", pkg.name)
                                        } else {
                                            format!("Dépendance retirée : {}", pkg.name)
                                        }
                                    })
                                    .collect();
                                if items.is_empty() {
                                    items.push(format!("Paquet DEB : {}", installation.native_id));
                                }
                                for replacement in &sim.preview.newly_installed_packages {
                                    items.push(format!(
                                        "Installation de remplacement APT : {replacement}"
                                    ));
                                }

                                let (blocked, reason) = if sim.is_essential_or_protected
                                    || !sim.protected_in_cascade.is_empty()
                                {
                                    let protected_list = if sim.protected_in_cascade.is_empty() {
                                        installation.native_id.clone()
                                    } else {
                                        sim.protected_in_cascade.join(", ")
                                    };
                                    (
                                        true,
                                        Some(format!(
                                            "Cette suppression affecterait des composants système protégés ({protected_list}) et est bloquée par sécurité."
                                        )),
                                    )
                                } else if has_blockers {
                                    let blockers = sim
                                        .preview
                                        .blocking_dependents
                                        .iter()
                                        .take(6)
                                        .cloned()
                                        .collect::<Vec<_>>()
                                        .join(", ");
                                    (
                                        true,
                                        Some(format!(
                                            "APT bloque la suppression en raison de dépendances requises par : {blockers}."
                                        )),
                                    )
                                } else {
                                    (false, None)
                                };

                                let dep_count = sim.preview.dependent_packages.len();
                                let cascade_note = if dep_count > 0 {
                                    format!(
                                        " Attention : {dep_count} autre{} paquet{} dépendant{} ser{} retiré{} par APT (autoremove désactivé).",
                                        if dep_count > 1 { "s" } else { "" },
                                        if dep_count > 1 { "s" } else { "" },
                                        if dep_count > 1 { "s" } else { "" },
                                        if dep_count > 1 { "ont" } else { "a" },
                                        if dep_count > 1 { "s" } else { "" },
                                    )
                                } else {
                                    " Aucun autre paquet dépendant ne sera retiré (autoremove désactivé).".into()
                                };
                                let impact = if options.purge_config {
                                    format!(
                                        "Le paquet sera supprimé avec ses fichiers de configuration système (/etc). Les fichiers personnels ne sont pas affectés.{cascade_note}"
                                    )
                                } else {
                                    format!(
                                        "Le paquet sera retiré. Ses fichiers de configuration système sont conservés pour une réinstallation ultérieure.{cascade_note}"
                                    )
                                };
                                deb_preview = Some(sim.preview);
                                (true, items, impact, blocked, reason)
                            }
                            Err(err) => (
                                true,
                                vec![format!("Paquet DEB : {}", installation.native_id)],
                                "Impossible de calculer l’aperçu APT.".into(),
                                true,
                                Some(err.message),
                            ),
                        }
                    }
                }
                PackageFormat::Snap => {
                    validate_snap_name(&installation.native_id)?;
                    let items = vec![format!("Snap : {}", installation.native_id)];
                    let (blocked, reason) = if is_protected_snap(&installation.native_id) {
                        (
                            true,
                            Some(
                                "Ce composant Snap système est protégé et ne peut pas être retiré."
                                    .into(),
                            ),
                        )
                    } else if !Path::new(SNAP_BIN).exists() {
                        (
                            true,
                            Some("Le gestionnaire Snap est indisponible sur ce système.".into()),
                        )
                    } else {
                        (false, None)
                    };
                    let impact =
                        "Le paquet Snap sera supprimé. Un instantané (snapshot) des données est automatiquement sauvegardé par snapd."
                            .into();
                    (true, items, impact, blocked, reason)
                }
                PackageFormat::Flatpak => {
                    let ref_label = installation
                        .reference
                        .clone()
                        .unwrap_or_else(|| installation.native_id.clone());
                    validate_flatpak_ref(&ref_label)?;
                    if let Some(ref inst) = installation.installation {
                        if !inst.is_empty() && inst != "user" {
                            validate_flatpak_installation(inst)?;
                        }
                    }
                    let scope_desc = match installation.installation.as_deref() {
                        Some(inst)
                            if installation.scope != "user"
                                && !inst.is_empty()
                                && inst != "default"
                                && inst != "system" =>
                        {
                            format!("system · {inst}")
                        }
                        _ => installation.scope.clone(),
                    };
                    let items = vec![format!("Flatpak ({scope_desc}) : {ref_label}")];
                    let impact = if options.remove_data {
                        "L’application Flatpak et ses données utilisateur stockées dans ~/.var/app seront supprimées."
                            .into()
                    } else {
                        "L’application Flatpak sera désinstallée. Les données utilisateur dans ~/.var/app restent intactes."
                            .into()
                    };
                    let (blocked, reason) = if !Path::new(FLATPAK_BIN).exists() {
                        (
                            true,
                            Some("Le gestionnaire Flatpak est indisponible sur ce système.".into()),
                        )
                    } else {
                        (false, None)
                    };
                    (installation.scope != "user", items, impact, blocked, reason)
                }
                PackageFormat::Appimage => {
                    let mut items = Vec::new();
                    let (blocked, reason) = if let Some(ref path_str) = installation.path {
                        let path = PathBuf::from(path_str);
                        if let Err(err) = validate_safe_user_file_path(&path) {
                            (true, Some(err.message))
                        } else if !path.exists() {
                            (
                                true,
                                Some(
                                    "Le fichier AppImage est introuvable à cet emplacement (fichier déplacé ou support déconnecté)."
                                        .into(),
                                ),
                            )
                        } else {
                            (false, None)
                        }
                    } else {
                        (
                            true,
                            Some("Le chemin du fichier AppImage n’est pas spécifié.".into()),
                        )
                    };

                    if let Some(ref path) = installation.path {
                        items.push(format!("Fichier exécutable : {path}"));
                        for desktop_file in find_associated_desktop_entries(path) {
                            items.push(format!("Raccourci de bureau : {}", desktop_file.display()));
                        }
                    } else {
                        items.push(format!("AppImage : {}", installation.name));
                    }
                    let impact =
                        "Le fichier autonome AppImage sera supprimé du disque. Aucun registre système n'est modifié."
                            .into();
                    (false, items, impact, blocked, reason)
                }
                PackageFormat::Local => {
                    let mut items = Vec::new();
                    let (blocked, reason) = if installation.scope != "user" {
                        (
                            true,
                            Some(
                                "Installation manuelle système (/opt ou /usr/local) : retirez-la via son propre désinstalleur ou manuellement par sécurité."
                                    .into(),
                            ),
                        )
                    } else if let Some(ref dir_str) = installation.installation {
                        let dir = PathBuf::from(dir_str);
                        if let Err(err) = validate_safe_local_install_dir(&dir) {
                            (true, Some(err.message))
                        } else if !dir.exists() {
                            (
                                true,
                                Some(
                                    "Le dossier d’installation local est introuvable (déjà supprimé ou déplacé)."
                                        .into(),
                                ),
                            )
                        } else if let Some(ref launcher_str) = installation.reference {
                            if let Err(err) =
                                validate_safe_user_desktop_launcher(Path::new(launcher_str))
                            {
                                (true, Some(err.message))
                            } else {
                                (false, None)
                            }
                        } else {
                            (false, None)
                        }
                    } else if let Some(ref path_str) = installation.path {
                        let exe = PathBuf::from(path_str);
                        if let Err(err) = validate_safe_user_file_path(&exe) {
                            (true, Some(err.message))
                        } else if !exe.exists() {
                            (
                                true,
                                Some(
                                    "Le fichier exécutable local est introuvable à cet emplacement."
                                        .into(),
                                ),
                            )
                        } else {
                            (false, None)
                        }
                    } else {
                        (
                            true,
                            Some(
                                "L’emplacement de l’installation locale n’est pas spécifié.".into(),
                            ),
                        )
                    };

                    if let Some(ref dir) = installation.installation {
                        items.push(format!("Dossier d’installation : {dir}"));
                    } else if let Some(ref exe) = installation.path {
                        items.push(format!("Fichier exécutable : {exe}"));
                    }
                    if let Some(ref launcher) = installation.reference {
                        items.push(format!("Lanceur de bureau : {launcher}"));
                    }
                    if items.is_empty() {
                        items.push(format!("Application locale : {}", installation.name));
                    }

                    let impact = "Le dossier (ou binaire) d’installation local et son lanceur .desktop seront supprimés. Vos données dans ~/.config ou vos documents personnels ne sont pas supprimés automatiquement.".into();
                    (false, items, impact, blocked, reason)
                }
            };

        let target_path = match installation.format {
            PackageFormat::Appimage | PackageFormat::Local => installation.path.clone(),
            PackageFormat::Flatpak => installation
                .reference
                .clone()
                .or_else(|| Some(installation.native_id.clone())),
            _ => None,
        };

        let estimated_freed_space = deb_preview
            .as_ref()
            .and_then(|p| p.total_freed_bytes)
            .map(format_bytes_fr)
            .or_else(|| installation.reported_size.clone())
            .or_else(|| installation.installed_size.map(format_bytes_fr));

        let plan = UninstallPlan {
            id: plan_id.clone(),
            target_id: installation.id.clone(),
            target_name: installation.name.clone(),
            format: installation.format,
            scope: installation.scope.clone(),
            native_id: installation.native_id.clone(),
            target_path,
            target_installation: installation.installation.clone(),
            options,
            affected_items,
            deb_preview,
            data_impact,
            estimated_freed_space,
            requires_auth,
            is_blocked,
            blocked_reason,
            created_at: now,
            expires_at: now + 300, // 5 minutes validity
        };

        let mut plans_guard = self.plans.lock().unwrap();
        // Prune expired plans
        plans_guard.retain(|_, p| p.expires_at >= now);
        plans_guard.insert(plan_id, plan.clone());

        Ok(plan)
    }

    pub fn confirm(&self, plan_id: &str) -> Result<Operation, AppError> {
        let now = current_timestamp();

        // 1. Check if another operation is running
        {
            let op_guard = self.current_operation.lock().unwrap();
            if let Some(ref op) = *op_guard {
                if matches!(
                    op.status,
                    OperationStatus::Running | OperationStatus::Verifying
                ) {
                    return Err(AppError::new(
                        "TRANSACTION_IN_PROGRESS",
                        "Une opération de désinstallation est déjà en cours d’exécution.",
                    ));
                }
            }
        }

        // 2. Consume plan atomically
        let plan = {
            let mut plans_guard = self.plans.lock().unwrap();
            plans_guard.remove(plan_id).ok_or_else(|| {
                AppError::new(
                    "PLAN_NOT_FOUND",
                    "Le plan de désinstallation est introuvable, a expiré ou a déjà été exécuté.",
                )
            })?
        };

        if plan.expires_at < now {
            return Err(AppError::new(
                "PLAN_EXPIRED",
                "Le plan de désinstallation a expiré. Préparez un nouvel aperçu.",
            ));
        }

        if plan.is_blocked {
            return Err(AppError::new(
                "OPERATION_BLOCKED",
                plan.blocked_reason
                    .as_deref()
                    .unwrap_or("Cette opération est bloquée pour la sécurité du système."),
            ));
        }

        // 3. Create active operation
        let op_counter = OP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let op_id = format!("op_{}_{}_{}", plan.id, now, op_counter);
        let mut operation = Operation {
            id: op_id.clone(),
            plan_id: plan.id.clone(),
            target_id: plan.target_id.clone(),
            target_name: plan.target_name.clone(),
            format: plan.format,
            status: OperationStatus::Running,
            step: "Exécution du retrait".into(),
            progress: None,
            message: format!("Suppression de {} en cours…", plan.target_name),
            error: None,
            started_at: now,
            finished_at: None,
        };

        {
            let mut op_guard = self.current_operation.lock().unwrap();
            *op_guard = Some(operation.clone());
        }

        // Persist in-flight state in SQLite so an abrupt crash can be reconciled on restart
        let _ = record_in_flight_operation(&InFlightOperation {
            operation_id: op_id.clone(),
            plan_id: plan.id.clone(),
            target_id: plan.target_id.clone(),
            target_name: plan.target_name.clone(),
            native_id: plan.native_id.clone(),
            format: plan.format,
            scope: plan.scope.clone(),
            target_path: plan.target_path.clone(),
            target_installation: plan.target_installation.clone(),
            started_at: now,
        });

        // 4. Execute operation
        let exec_result = self.execute_plan(&plan);

        let _ = remove_in_flight_operation(&op_id);

        // 5. Update operation state
        let finish_time = current_timestamp();
        let (final_status, final_step, final_msg, app_err) = match exec_result {
            Ok(msg) => (OperationStatus::Completed, "Terminé", msg, None),
            Err((OperationStatus::Cancelled, err)) => (
                OperationStatus::Cancelled,
                "Annulé",
                err.message.clone(),
                Some(err),
            ),
            Err((_, err)) => (
                OperationStatus::Failed,
                "Échec",
                format!("Échec de la désinstallation : {}", err.message),
                Some(err),
            ),
        };

        operation.status = final_status;
        operation.step = final_step.into();
        operation.message = final_msg.clone();
        operation.error = app_err.clone();
        operation.finished_at = Some(finish_time);

        {
            let mut op_guard = self.current_operation.lock().unwrap();
            *op_guard = Some(operation.clone());
        }

        // 6. Record to history
        let history_entry = HistoryEntry {
            id: format!("hist_{}", op_id),
            operation_id: op_id,
            target_name: plan.target_name,
            native_id: plan.native_id,
            format: plan.format,
            status: final_status,
            summary: final_msg,
            diagnostic: app_err
                .as_ref()
                .map(|e| format!("{}: {}", e.code, e.message))
                .unwrap_or_default(),
            timestamp: finish_time,
        };
        let _ = append_history(history_entry);

        if final_status == OperationStatus::Failed {
            if let Some(err) = app_err {
                return Err(err);
            }
        }
        Ok(operation)
    }

    fn execute_plan(&self, plan: &UninstallPlan) -> Result<String, (OperationStatus, AppError)> {
        match plan.format {
            PackageFormat::Appimage => self
                .execute_appimage_removal(plan)
                .map_err(|e| (OperationStatus::Failed, e)),
            PackageFormat::Local => self
                .execute_local_removal(plan)
                .map_err(|e| (OperationStatus::Failed, e)),
            PackageFormat::Flatpak => self.execute_flatpak_removal(plan),
            PackageFormat::Deb => self.execute_deb_removal(plan),
            PackageFormat::Snap => self.execute_snap_removal(plan),
        }
    }

    fn execute_local_removal(&self, plan: &UninstallPlan) -> Result<String, AppError> {
        if plan.scope != "user" {
            return Err(AppError::new(
                "OPERATION_BLOCKED",
                "La suppression automatique est limitée aux installations locales dans votre dossier personnel.",
            ));
        }

        let launcher_path = plan
            .affected_items
            .iter()
            .find_map(|item| item.strip_prefix("Lanceur de bureau : "))
            .map(PathBuf::from);

        if let Some(ref launcher) = launcher_path {
            validate_safe_user_desktop_launcher(launcher)?;
        }

        if let Some(ref dir_str) = plan.target_installation {
            let dir = PathBuf::from(dir_str);
            validate_safe_local_install_dir(&dir)?;
            if !dir.exists() {
                return Err(AppError::new(
                    "LOCAL_TARGET_NOT_FOUND",
                    "Le dossier d’installation local n’existe plus à cet emplacement.",
                ));
            }
            fs::remove_dir_all(&dir).map_err(|e| {
                AppError::new(
                    "LOCAL_REMOVE_FAILED",
                    &format!("Impossible de supprimer le dossier local : {e}"),
                )
            })?;
            if dir.symlink_metadata().is_ok() {
                return Err(AppError::new(
                    "LOCAL_VERIFY_FAILED",
                    "Le dossier d’installation local est toujours présent après suppression.",
                ));
            }
        } else if let Some(ref exe_str) = plan.target_path {
            let exe = PathBuf::from(exe_str);
            validate_safe_user_file_path(&exe)?;
            if !exe.exists() {
                return Err(AppError::new(
                    "LOCAL_TARGET_NOT_FOUND",
                    "Le fichier exécutable local n’existe plus à cet emplacement.",
                ));
            }
            fs::remove_file(&exe).map_err(|e| {
                AppError::new(
                    "LOCAL_REMOVE_FAILED",
                    &format!("Impossible de supprimer le fichier exécutable local : {e}"),
                )
            })?;
            if exe.symlink_metadata().is_ok() {
                return Err(AppError::new(
                    "LOCAL_VERIFY_FAILED",
                    "Le fichier exécutable local est toujours présent après suppression.",
                ));
            }
        } else {
            return Err(AppError::new(
                "LOCAL_NO_PATH",
                "Aucun chemin d’installation local n’est associé à ce plan.",
            ));
        }

        if let Some(ref launcher) = launcher_path {
            if launcher.symlink_metadata().is_ok() {
                fs::remove_file(launcher).map_err(|e| {
                    AppError::new(
                        "LOCAL_LAUNCHER_REMOVE_FAILED",
                        &format!("Application retirée mais échec de suppression du lanceur : {e}"),
                    )
                })?;
            }
        }

        Ok(format!(
            "L’application locale {} a été supprimée avec succès.",
            plan.target_name
        ))
    }

    fn execute_appimage_removal(&self, plan: &UninstallPlan) -> Result<String, AppError> {
        let raw_path = plan
            .target_path
            .as_deref()
            .or_else(|| {
                plan.affected_items
                    .iter()
                    .find_map(|item| item.strip_prefix("Fichier exécutable : "))
            })
            .ok_or_else(|| {
                AppError::new(
                    "APPIMAGE_NO_PATH",
                    "Le chemin du fichier AppImage est introuvable.",
                )
            })?;

        let path = PathBuf::from(raw_path);
        validate_safe_user_file_path(&path)?;

        if !path.exists() {
            return Err(AppError::new(
                "APPIMAGE_FILE_NOT_FOUND",
                "Le fichier AppImage n’existe plus à cet emplacement.",
            ));
        }

        // Identify associated desktop entries BEFORE deleting the executable file
        // so that canonicalize() comparisons can resolve symlinks on both sides!
        let associated_desktop_files = find_associated_desktop_entries(raw_path);

        // Delete the executable file
        fs::remove_file(&path).map_err(|e| {
            AppError::new(
                "APPIMAGE_REMOVE_FAILED",
                &format!("Impossible de supprimer le fichier AppImage : {e}"),
            )
        })?;

        // Clean up all associated .desktop files if present
        for desktop_path in associated_desktop_files {
            let _ = fs::remove_file(desktop_path);
        }

        // Verify removal (using symlink_metadata so symlinks are correctly verified)
        if path.symlink_metadata().is_ok() {
            return Err(AppError::new(
                "APPIMAGE_VERIFY_FAILED",
                "Le fichier est toujours présent après la tentative de suppression.",
            ));
        }

        Ok(format!(
            "Le fichier AppImage {} a été supprimé avec succès.",
            plan.target_name
        ))
    }

    fn execute_flatpak_removal(
        &self,
        plan: &UninstallPlan,
    ) -> Result<String, (OperationStatus, AppError)> {
        let target_ref = plan
            .target_path
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or(&plan.native_id);

        validate_flatpak_ref(target_ref).map_err(|e| (OperationStatus::Failed, e))?;

        if plan.scope == "user" {
            let mut args = vec!["uninstall", "--user", "-y", "--noninteractive"];
            if plan.options.remove_data {
                args.push("--delete-data");
            }
            args.push("--");
            args.push(target_ref);

            let flatpak_bin = if Path::new(FLATPAK_BIN).exists() {
                FLATPAK_BIN
            } else {
                "flatpak"
            };

            let output = std::process::Command::new(flatpak_bin)
                .args(&args)
                .output()
                .map_err(|e| {
                    (
                        OperationStatus::Failed,
                        AppError::new(
                            "FLATPAK_EXEC_FAILED",
                            &format!("Impossible d’exécuter la commande Flatpak : {e}"),
                        ),
                    )
                })?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err((
                    OperationStatus::Failed,
                    AppError::new(
                        "FLATPAK_UNINSTALL_ERROR",
                        &format!("Erreur lors de la désinstallation Flatpak : {stderr}"),
                    ),
                ));
            }

            verify_flatpak_removed(target_ref, "--user")
                .map_err(|e| (OperationStatus::Failed, e))?;

            Ok(format!(
                "L’application Flatpak {} a été retirée avec succès.",
                plan.target_name
            ))
        } else {
            let (pkexec_args, scope_flag) = build_flatpak_system_pkexec_args(
                target_ref,
                plan.target_installation.as_deref(),
                plan.options.remove_data,
            )
            .map_err(|e| (OperationStatus::Failed, e))?;

            run_pkexec(&pkexec_args, "Flatpak")?;
            verify_flatpak_removed(target_ref, &scope_flag)
                .map_err(|e| (OperationStatus::Failed, e))?;

            Ok(format!(
                "L’application Flatpak système {} a été retirée avec succès.",
                plan.target_name
            ))
        }
    }

    fn execute_deb_removal(
        &self,
        plan: &UninstallPlan,
    ) -> Result<String, (OperationStatus, AppError)> {
        validate_deb_package_name(&plan.native_id).map_err(|e| (OperationStatus::Failed, e))?;

        if is_protected_deb(&plan.native_id) {
            return Err((
                OperationStatus::Failed,
                AppError::new(
                    "OPERATION_BLOCKED",
                    "Ce paquet est essentiel au fonctionnement du système Ubuntu et ne peut pas être supprimé.",
                ),
            ));
        }

        if is_dpkg_locked() {
            return Err((
                OperationStatus::Failed,
                AppError::new(
                    "MANAGER_LOCKED",
                    "Le gestionnaire de paquets APT/dpkg est actuellement verrouillé par une autre opération système. Veuillez réessayer ultérieurement.",
                ),
            ));
        }

        // Re-evaluate consequences right before privileged execution
        let simulation = simulate_deb_removal(&plan.native_id, plan.options.purge_config)
            .map_err(|e| (OperationStatus::Failed, e))?;

        if !simulation.is_installed {
            return Err((
                OperationStatus::Failed,
                AppError::new(
                    "TARGET_NOT_FOUND",
                    "Le paquet DEB ciblé n’est plus installé sur le système.",
                ),
            ));
        }

        if simulation.is_essential_or_protected
            || !simulation.protected_in_cascade.is_empty()
            || !simulation.preview.blocking_dependents.is_empty()
        {
            return Err((
                OperationStatus::Failed,
                AppError::new(
                    "OPERATION_BLOCKED",
                    "Cette suppression affecterait des composants système protégés ou des dépendances bloquantes et a été stoppée avant exécution.",
                ),
            ));
        }

        if deb_plan_packages_changed(
            plan,
            &simulation.packages_to_remove,
            &simulation.preview.newly_installed_packages,
        ) {
            return Err((
                OperationStatus::Failed,
                AppError::new(
                    "PLAN_CHANGED",
                    "Les paquets impactés ont changé depuis l’aperçu. Veuillez générer un nouvel aperçu avant de confirmer.",
                ),
            ));
        }

        let pkexec_args = build_deb_pkexec_args(&plan.native_id, plan.options.purge_config)
            .map_err(|e| (OperationStatus::Failed, e))?;

        run_pkexec(&pkexec_args, "APT")?;
        verify_deb_removed(&plan.native_id).map_err(|e| (OperationStatus::Failed, e))?;

        Ok(format!(
            "Le paquet DEB {} a été retiré avec succès.",
            plan.target_name
        ))
    }

    fn execute_snap_removal(
        &self,
        plan: &UninstallPlan,
    ) -> Result<String, (OperationStatus, AppError)> {
        validate_snap_name(&plan.native_id).map_err(|e| (OperationStatus::Failed, e))?;

        if is_protected_snap(&plan.native_id) {
            return Err((
                OperationStatus::Failed,
                AppError::new(
                    "OPERATION_BLOCKED",
                    "Ce composant Snap système est protégé et ne peut pas être retiré.",
                ),
            ));
        }

        let pkexec_args =
            build_snap_pkexec_args(&plan.native_id).map_err(|e| (OperationStatus::Failed, e))?;

        run_pkexec(&pkexec_args, "Snap")?;
        verify_snap_removed(&plan.native_id).map_err(|e| (OperationStatus::Failed, e))?;

        Ok(format!(
            "Le paquet Snap {} a été retiré avec succès.",
            plan.target_name
        ))
    }

    pub fn get_current_operation(&self) -> Option<Operation> {
        self.current_operation.lock().unwrap().clone()
    }
}

/// Reconciles operations that were interrupted by an abrupt app exit against the real
/// local system state. Never invents a supposed success.
pub fn reconcile_interrupted_operations() -> usize {
    let pending = list_in_flight_operations();
    let count = pending.len();
    let now = current_timestamp();

    for op in pending {
        let local_state_note = match op.format {
            PackageFormat::Appimage => match op.target_path.as_deref() {
                Some(path) if Path::new(path).symlink_metadata().is_ok() => {
                    "le fichier AppImage est toujours présent sur le disque"
                }
                Some(_) => "le fichier AppImage n’est plus présent sur le disque",
                None => "chemin de fichier inconnu",
            },
            PackageFormat::Deb => match query_deb_status(&op.native_id) {
                Ok((true, _)) => "le paquet DEB est toujours installé sur le système",
                Ok((false, _)) => "le paquet DEB n’est plus détecté comme installé",
                Err(_) => "état réel DEB non vérifiable automatiquement",
            },
            PackageFormat::Flatpak => {
                let target_ref = op
                    .target_path
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(&op.native_id);
                let scope_flag = if op.scope == "user" {
                    Ok("--user".to_string())
                } else {
                    flatpak_system_scope_flag(op.target_installation.as_deref())
                };
                match scope_flag {
                    Ok(flag) => match verify_flatpak_removed(target_ref, &flag) {
                        Ok(()) => "l’application Flatpak n’est plus détectée",
                        Err(_) => "l’application Flatpak est toujours détectée sur le système",
                    },
                    Err(_) => "état réel Flatpak non vérifiable automatiquement",
                }
            }
            PackageFormat::Snap => match verify_snap_removed(&op.native_id) {
                Ok(()) => "le paquet Snap n’est plus détecté sur le système",
                Err(_) => "le paquet Snap est toujours détecté sur le système",
            },
            PackageFormat::Local => {
                let target = op
                    .target_installation
                    .as_deref()
                    .or(op.target_path.as_deref());
                match target {
                    Some(path) if Path::new(path).symlink_metadata().is_ok() => {
                        "l’installation locale est toujours présente sur le disque"
                    }
                    Some(_) => "l’installation locale n’est plus présente sur le disque",
                    None => "chemin local inconnu",
                }
            }
        };

        let summary = format!(
            "Opération interrompue avant sa fin — état local vérifié au redémarrage : {local_state_note}."
        );
        let entry = HistoryEntry {
            id: format!("hist_{}", op.operation_id),
            operation_id: op.operation_id.clone(),
            target_name: op.target_name,
            native_id: op.native_id,
            format: op.format,
            status: OperationStatus::Failed,
            summary,
            diagnostic: format!("OPERATION_INTERRUPTED: {local_state_note}"),
            timestamp: now,
        };
        let _ = append_history(entry);
        let _ = remove_in_flight_operation(&op.operation_id);
    }

    count
}

pub(crate) fn deb_plan_packages_changed(
    plan: &UninstallPlan,
    simulated_removed: &[String],
    simulated_installed: &[String],
) -> bool {
    let mut planned_removed: HashSet<&str> = plan
        .affected_items
        .iter()
        .filter_map(|item| {
            item.strip_prefix("Paquet DEB : ")
                .or_else(|| item.strip_prefix("Dépendance retirée : "))
        })
        .collect();
    let mut planned_installed: HashSet<&str> = plan
        .affected_items
        .iter()
        .filter_map(|item| item.strip_prefix("Installation de remplacement APT : "))
        .collect();

    if let Some(ref preview) = plan.deb_preview {
        for pkg in &preview.removed_packages {
            planned_removed.insert(pkg.name.as_str());
        }
        for pkg in &preview.newly_installed_packages {
            planned_installed.insert(pkg.as_str());
        }
    }

    if !planned_removed.is_empty()
        && simulated_removed
            .iter()
            .any(|pkg| !planned_removed.contains(pkg.as_str()))
    {
        return true;
    }

    simulated_installed
        .iter()
        .any(|pkg| !planned_installed.contains(pkg.as_str()))
}

pub fn validate_safe_local_install_dir(path: &Path) -> Result<(), AppError> {
    if !path.is_absolute() {
        return Err(AppError::new(
            "INVALID_PATH",
            "Le chemin du dossier d’installation doit être absolu.",
        ));
    }
    let path_str = path.to_string_lossy();
    if path_str.contains('\0') || path_str.contains("..") {
        return Err(AppError::new(
            "INVALID_PATH",
            "Le chemin contient des séquences non autorisées.",
        ));
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .ok_or_else(|| {
            AppError::new(
                "UNSAFE_LOCATION",
                "Impossible de vérifier le dossier personnel.",
            )
        })?;
    let canonical_home = home.canonicalize().unwrap_or(home);
    let canonical_dir = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if !canonical_dir.starts_with(&canonical_home) {
        return Err(AppError::new(
            "UNSAFE_LOCATION",
            "Le dossier d’installation doit se trouver dans votre dossier personnel.",
        ));
    }
    let expected = crate::providers::local::infer_app_install_root(
        &canonical_dir.join("_probe"),
        &canonical_home,
    );
    if expected.as_deref() != Some(canonical_dir.as_path()) {
        return Err(AppError::new(
            "FORBIDDEN_PATH",
            "Ce dossier est un répertoire partagé ou protégé et ne peut pas être supprimé.",
        ));
    }
    Ok(())
}

pub fn validate_safe_user_desktop_launcher(path: &Path) -> Result<(), AppError> {
    if !path.is_absolute() || path.extension().is_none_or(|ext| ext != "desktop") || path.is_dir() {
        return Err(AppError::new(
            "INVALID_PATH",
            "Le chemin du lanceur .desktop est invalide.",
        ));
    }
    let path_str = path.to_string_lossy();
    if path_str.contains('\0') || path_str.contains("..") {
        return Err(AppError::new(
            "INVALID_PATH",
            "Le chemin du lanceur .desktop contient des séquences non autorisées.",
        ));
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .ok_or_else(|| {
            AppError::new(
                "UNSAFE_LOCATION",
                "Impossible de vérifier le dossier personnel.",
            )
        })?;
    let mut allowed_dirs = vec![home.join(".local/share/applications")];
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
    {
        allowed_dirs.push(xdg.join("applications"));
    }
    let parent = path
        .parent()
        .and_then(|p| p.canonicalize().ok().or_else(|| Some(p.to_path_buf())))
        .ok_or_else(|| {
            AppError::new(
                "INVALID_PATH",
                "Le dossier parent du lanceur .desktop est invalide.",
            )
        })?;
    let in_allowed = allowed_dirs.into_iter().any(|dir| {
        let canonical_allowed = dir.canonicalize().unwrap_or(dir);
        parent == canonical_allowed
    });
    if !in_allowed {
        return Err(AppError::new(
            "FORBIDDEN_PATH",
            "Seuls les lanceurs .desktop de votre dossier utilisateur peuvent être retirés.",
        ));
    }
    Ok(())
}

pub fn validate_safe_user_file_path(path: &Path) -> Result<(), AppError> {
    if !path.is_absolute() {
        return Err(AppError::new("INVALID_PATH", "Le chemin doit être absolu."));
    }

    let path_str = path.to_string_lossy();
    if path_str.contains('\0') || path_str.contains("..") {
        return Err(AppError::new(
            "INVALID_PATH",
            "Le chemin contient des séquences non autorisées.",
        ));
    }

    if path.is_dir() {
        return Err(AppError::new(
            "INVALID_PATH",
            "Le chemin spécifié est un répertoire, pas un fichier.",
        ));
    }

    // Safety: never allow deleting directly from root or vital system directories
    for forbidden in &[
        "/bin", "/sbin", "/usr", "/etc", "/lib", "/lib64", "/sys", "/proc", "/dev", "/boot",
        "/root", "/var", "/opt", "/srv",
    ] {
        if path.starts_with(Path::new(forbidden)) {
            return Err(AppError::new(
                "FORBIDDEN_PATH",
                "Interdiction de supprimer un fichier dans les répertoires système.",
            ));
        }
    }

    // Must be in a user directory (e.g. /home, /tmp, /run/user, /media, /mnt)
    let home = std::env::var("HOME").unwrap_or_default();
    let is_in_home = if !home.is_empty() {
        path.starts_with(Path::new(&home))
    } else {
        false
    };
    let is_in_tmp_or_mount = path.starts_with(Path::new("/tmp"))
        || path.starts_with(Path::new("/run/user"))
        || path.starts_with(Path::new("/media"))
        || path.starts_with(Path::new("/mnt"));

    if !is_in_home && !is_in_tmp_or_mount {
        return Err(AppError::new(
            "UNSAFE_LOCATION",
            "Le fichier doit se trouver dans votre dossier personnel ou un support utilisateur.",
        ));
    }

    // Defense-in-depth: check if canonicalized path (resolving symlinks) points to forbidden system directories
    // or targets a foreign directory outside user allowed locations
    if let Ok(canonical) = path.canonicalize() {
        for forbidden in &[
            "/bin", "/sbin", "/usr", "/etc", "/lib", "/lib64", "/sys", "/proc", "/dev", "/boot",
            "/root", "/var", "/opt", "/srv",
        ] {
            if canonical.starts_with(Path::new(forbidden)) {
                return Err(AppError::new(
                    "FORBIDDEN_PATH",
                    "Le lien symbolique cible un répertoire système protégé.",
                ));
            }
        }

        let canonical_in_home = if !home.is_empty() {
            canonical.starts_with(Path::new(&home))
        } else {
            false
        };
        let canonical_in_tmp_or_mount = canonical.starts_with(Path::new("/tmp"))
            || canonical.starts_with(Path::new("/run/user"))
            || canonical.starts_with(Path::new("/media"))
            || canonical.starts_with(Path::new("/mnt"));

        if !canonical_in_home && !canonical_in_tmp_or_mount {
            return Err(AppError::new(
                "UNSAFE_LOCATION",
                "Le lien symbolique cible un emplacement non autorisé en dehors de votre dossier personnel.",
            ));
        }
    }

    Ok(())
}

pub(crate) fn is_desktop_associated_with_appimage(content: &str, appimage_path: &Path) -> bool {
    let mut in_entry = false;
    let mut exec_val = None;
    for line in content.lines().map(str::trim) {
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry {
            if let Some((key, value)) = line.split_once('=') {
                if key.trim() == "Exec" {
                    exec_val = Some(value.trim());
                    break;
                }
            }
        }
    }

    let Some(exec) = exec_val else {
        return false;
    };

    let Some(tokens) = crate::providers::appimage::exec_tokens(exec) else {
        return false;
    };

    let mut tokens = tokens.iter();
    let mut program = match tokens.next() {
        Some(p) => p,
        None => return false,
    };
    if program == "env" || program.ends_with("/env") {
        let mut target = None;
        let mut skip_next = false;
        for token in tokens {
            if skip_next {
                skip_next = false;
                continue;
            }
            if token == "-u"
                || token == "--unset"
                || token == "-C"
                || token == "--chdir"
                || token == "-S"
                || token == "--split-string"
            {
                skip_next = true;
                continue;
            }
            if token.starts_with('-') || token.contains('=') {
                continue;
            }
            target = Some(token);
            break;
        }
        program = match target {
            Some(p) => p,
            None => return false,
        };
    }

    let prog_path = if let Some(suffix) = program.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            PathBuf::from(home).join(suffix)
        } else {
            PathBuf::from(program)
        }
    } else {
        PathBuf::from(program)
    };

    if prog_path == appimage_path {
        return true;
    }

    if let Ok(c1) = prog_path.canonicalize() {
        if c1 == appimage_path {
            return true;
        }
        if let Ok(c2) = appimage_path.canonicalize() {
            if c1 == c2 {
                return true;
            }
        }
    } else if let Ok(c2) = appimage_path.canonicalize() {
        if prog_path == c2 {
            return true;
        }
    }

    false
}

fn find_associated_desktop_entries(appimage_raw_path: &str) -> Vec<PathBuf> {
    let mut results = Vec::new();
    let appimage_path = Path::new(appimage_raw_path);
    let mut search_dirs = Vec::new();

    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        let xdg_path = PathBuf::from(xdg);
        if xdg_path.is_absolute() {
            let p = xdg_path.join("applications");
            if p.exists() {
                search_dirs.push(p);
            }
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        let home_path = PathBuf::from(&home);
        if home_path.is_absolute() {
            let config_dir = std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .unwrap_or_else(|| home_path.join(".config"));

            if let Ok(user_dirs) = fs::read_to_string(config_dir.join("user-dirs.dirs")) {
                for line in user_dirs.lines() {
                    if let Some((key, val)) = line.trim().split_once('=') {
                        if key.trim() == "XDG_DESKTOP_DIR" {
                            let unquoted = val.trim().trim_matches('"');
                            let resolved = if let Some(suffix) = unquoted.strip_prefix("$HOME/") {
                                home_path.join(suffix)
                            } else if unquoted == "$HOME" {
                                home_path.clone()
                            } else {
                                PathBuf::from(unquoted)
                            };
                            if resolved.is_absolute()
                                && resolved.exists()
                                && !search_dirs.contains(&resolved)
                            {
                                search_dirs.push(resolved);
                            }
                        }
                    }
                }
            }

            let p = home_path.join(".local/share/applications");
            if p.exists() && !search_dirs.contains(&p) {
                search_dirs.push(p);
            }
            let bureau_dir = home_path.join("Bureau");
            if bureau_dir.exists() && !search_dirs.contains(&bureau_dir) {
                search_dirs.push(bureau_dir);
            }
            let desktop_dir = home_path.join("Desktop");
            if desktop_dir.exists() && !search_dirs.contains(&desktop_dir) {
                search_dirs.push(desktop_dir);
            }
        }
    }

    let mut seen = std::collections::HashSet::new();
    for dir in search_dirs {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|ext| ext.to_str()) == Some("desktop") {
                    if let Ok(content) = fs::read_to_string(&path) {
                        if is_desktop_associated_with_appimage(&content, appimage_path) {
                            let canonical_key =
                                path.canonicalize().unwrap_or_else(|_| path.clone());
                            if seen.insert(canonical_key) {
                                results.push(path);
                            }
                        }
                    }
                }
            }
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prevents_uninstalling_protected_package() {
        let manager = TransactionManager {
            plans: Mutex::new(HashMap::new()),
            current_operation: Mutex::new(None),
        };

        let installation = ApplicationInstallation {
            id: "deb:system:dpkg:amd64".into(),
            name: "dpkg".into(),
            native_id: "dpkg".into(),
            version: "1.22".into(),
            format: PackageFormat::Deb,
            scope: "system".into(),
            description: "".into(),
            installed_size: None,
            reported_size: None,
            installation: None,
            reference: None,
            path: None,
            is_executable: None,
            is_desktop_application: false,
            can_uninstall: false,
            unavailable_reason: "".into(),
        };

        let plan = manager
            .prepare(&installation, UninstallPlanOptions::default())
            .expect("prepare should return a plan");

        assert!(plan.is_blocked);
        assert!(plan.blocked_reason.is_some());

        // Attempting to confirm a blocked plan must fail!
        let result = manager.confirm(&plan.id);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, "OPERATION_BLOCKED");
    }

    #[test]
    fn single_use_and_anti_double_click_protection() {
        let _lock = crate::storage::TEST_STORAGE_LOCK.lock().unwrap();
        let temp_dir =
            std::env::temp_dir().join(format!("panel_center_test_trans_{}", std::process::id()));
        std::env::set_var("PANEL_CENTER_DATA_DIR", &temp_dir);
        std::env::set_var("PANEL_CENTER_CONFIG_DIR", &temp_dir);

        let manager = TransactionManager {
            plans: Mutex::new(HashMap::new()),
            current_operation: Mutex::new(None),
        };

        // Create a temporary AppImage file in /tmp
        let temp_file =
            std::env::temp_dir().join(format!("test_app_{}.AppImage", std::process::id()));
        fs::write(&temp_file, b"dummy appimage content").unwrap();

        let installation = ApplicationInstallation {
            id: format!("appimage:{}", temp_file.display()),
            name: "Test App".into(),
            native_id: "test-app".into(),
            version: "1.0".into(),
            format: PackageFormat::Appimage,
            scope: "user".into(),
            description: "".into(),
            installed_size: Some(22),
            reported_size: Some("22 B".into()),
            installation: None,
            reference: None,
            path: Some(temp_file.to_string_lossy().into()),
            is_executable: Some(true),
            is_desktop_application: true,
            can_uninstall: true,
            unavailable_reason: "".into(),
        };

        let plan = manager
            .prepare(&installation, UninstallPlanOptions::default())
            .expect("prepare");

        assert!(!plan.is_blocked);
        assert_eq!(plan.target_path, Some(temp_file.to_string_lossy().into()));

        // First confirm execution
        let op = manager.confirm(&plan.id).expect("first confirm succeeds");
        assert_eq!(op.status, OperationStatus::Completed);
        assert!(!temp_file.exists()); // File removed!

        // Second confirm execution with same plan_id (double click simulation) must fail!
        let second_result = manager.confirm(&plan.id);
        assert!(second_result.is_err());
        assert_eq!(second_result.unwrap_err().code, "PLAN_NOT_FOUND");

        let _ = fs::remove_dir_all(&temp_dir);
        std::env::remove_var("PANEL_CENTER_DATA_DIR");
        std::env::remove_var("PANEL_CENTER_CONFIG_DIR");
    }

    #[test]
    fn distinct_plan_ids_generated() {
        let manager = TransactionManager {
            plans: Mutex::new(HashMap::new()),
            current_operation: Mutex::new(None),
        };

        let installation = ApplicationInstallation {
            id: "deb:system:vlc:amd64".into(),
            name: "VLC".into(),
            native_id: "vlc".into(),
            version: "3.0.20".into(),
            format: PackageFormat::Deb,
            scope: "system".into(),
            description: "".into(),
            installed_size: None,
            reported_size: None,
            installation: None,
            reference: None,
            path: None,
            is_executable: None,
            is_desktop_application: true,
            can_uninstall: false,
            unavailable_reason: "".into(),
        };

        let plan1 = manager
            .prepare(&installation, UninstallPlanOptions::default())
            .unwrap();
        let plan2 = manager
            .prepare(&installation, UninstallPlanOptions::default())
            .unwrap();

        assert_ne!(plan1.id, plan2.id);
    }

    #[test]
    fn concurrent_operation_status_access() {
        let mgr = manager();
        // Check that reading operation does not panic or deadlock
        let _op = mgr.get_current_operation();
    }

    #[test]
    fn rejects_dangerous_paths() {
        assert!(validate_safe_user_file_path(Path::new("/bin/ls")).is_err());
        assert!(validate_safe_user_file_path(Path::new("/usr/bin/bash")).is_err());
        assert!(validate_safe_user_file_path(Path::new("/etc/passwd")).is_err());
        assert!(validate_safe_user_file_path(Path::new("relative/path")).is_err());

        // Reject directory paths
        let temp_dir = std::env::temp_dir();
        assert!(validate_safe_user_file_path(&temp_dir).is_err());
    }

    #[test]
    fn is_dpkg_locked_detection_runs_cleanly() {
        // Must return bool without panicking
        let _locked = is_dpkg_locked();
    }

    #[test]
    fn associated_desktop_entries_exact_match_only() {
        let app_path = Path::new("/tmp/test_unique_app.AppImage");

        // Exact match
        let entry1 =
            "[Desktop Entry]\nType=Application\nName=App\nExec=/tmp/test_unique_app.AppImage\n";
        assert!(is_desktop_associated_with_appimage(entry1, app_path));

        // Exact match with quotes and arguments
        let entry2 = "[Desktop Entry]\nType=Application\nName=App\nExec=\"/tmp/test_unique_app.AppImage\" --no-sandbox %U\n";
        assert!(is_desktop_associated_with_appimage(entry2, app_path));

        // Exact match with env wrapper
        let entry3 = "[Desktop Entry]\nType=Application\nName=App\nExec=env DESKTOPINTEGRATION=1 /tmp/test_unique_app.AppImage\n";
        assert!(is_desktop_associated_with_appimage(entry3, app_path));

        // False positive: prefix substring match (MUST BE REJECTED)
        let entry_prefix =
            "[Desktop Entry]\nType=Application\nName=App2\nExec=/tmp/test_unique_app.AppImage2\n";
        assert!(!is_desktop_associated_with_appimage(entry_prefix, app_path));

        // False positive: comment mention (MUST BE REJECTED)
        let entry_comment = "[Desktop Entry]\nType=Application\nName=Other\nComment=Replaces /tmp/test_unique_app.AppImage\nExec=/usr/bin/other\n";
        assert!(!is_desktop_associated_with_appimage(
            entry_comment,
            app_path
        ));
    }

    #[test]
    fn associated_desktop_entries_xdg_data_home_support() {
        let _lock = crate::storage::TEST_STORAGE_LOCK.lock().unwrap();
        let temp_dir = std::env::temp_dir().join(format!("panel_test_xdg_{}", std::process::id()));
        let apps_dir = temp_dir.join("applications");
        fs::create_dir_all(&apps_dir).unwrap();

        let desktop_file = apps_dir.join("test_target.desktop");
        let content = "[Desktop Entry]\nType=Application\nName=Test\nExec=/tmp/xdg_test.AppImage\n";
        fs::write(&desktop_file, content).unwrap();

        std::env::set_var("XDG_DATA_HOME", &temp_dir);

        let matches = find_associated_desktop_entries("/tmp/xdg_test.AppImage");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0], desktop_file);

        let _ = fs::remove_dir_all(&temp_dir);
        std::env::remove_var("XDG_DATA_HOME");
    }

    #[test]
    fn validate_safe_user_file_path_rejects_prefix_confusion() {
        // Must reject path that string-starts with /tmp but is not in /tmp component
        assert!(validate_safe_user_file_path(Path::new("/tmpx/evil.AppImage")).is_err());
        assert!(validate_safe_user_file_path(Path::new("/media_evil/evil.AppImage")).is_err());
        assert!(validate_safe_user_file_path(Path::new("/mnt_other/evil.AppImage")).is_err());

        // Must reject system paths /var, /opt, /srv
        assert!(validate_safe_user_file_path(Path::new("/var/evil.AppImage")).is_err());
        assert!(validate_safe_user_file_path(Path::new("/opt/evil.AppImage")).is_err());
        assert!(validate_safe_user_file_path(Path::new("/srv/evil.AppImage")).is_err());
    }

    #[test]
    fn associated_desktop_entries_supports_tilde_expansion() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/home/user".into());
        let target = PathBuf::from(&home).join("Applications/Test.AppImage");

        let entry =
            "[Desktop Entry]\nType=Application\nName=Test\nExec=~/Applications/Test.AppImage\n";
        assert!(is_desktop_associated_with_appimage(entry, &target));
    }

    #[test]
    fn prepare_blocks_nonexistent_or_unsafe_appimage() {
        let manager = TransactionManager {
            plans: Mutex::new(HashMap::new()),
            current_operation: Mutex::new(None),
        };

        // Nonexistent AppImage
        let mut installation = ApplicationInstallation {
            id: "appimage:/tmp/nonexistent_12345.AppImage".into(),
            name: "Missing".into(),
            native_id: "missing".into(),
            version: "1.0".into(),
            format: PackageFormat::Appimage,
            scope: "user".into(),
            description: "".into(),
            installed_size: None,
            reported_size: None,
            installation: None,
            reference: None,
            path: Some("/tmp/nonexistent_12345.AppImage".into()),
            is_executable: Some(true),
            is_desktop_application: true,
            can_uninstall: true,
            unavailable_reason: "".into(),
        };

        let plan = manager
            .prepare(&installation, UninstallPlanOptions::default())
            .expect("prepare returns plan");
        assert!(plan.is_blocked);
        assert!(plan.blocked_reason.unwrap().contains("introuvable"));

        // Dangerous system path AppImage
        installation.path = Some("/etc/shadow".into());
        let dangerous_plan = manager
            .prepare(&installation, UninstallPlanOptions::default())
            .expect("prepare returns plan");
        assert!(dangerous_plan.is_blocked);
        assert!(dangerous_plan
            .blocked_reason
            .unwrap()
            .contains("répertoires système"));
    }

    #[test]
    fn flatpak_plan_preserves_full_reference() {
        let manager = TransactionManager {
            plans: Mutex::new(HashMap::new()),
            current_operation: Mutex::new(None),
        };

        let installation = ApplicationInstallation {
            id: "flatpak:user:default:app/org.videolan.VLC/x86_64/beta".into(),
            name: "VLC Beta".into(),
            native_id: "org.videolan.VLC".into(),
            version: "4.0".into(),
            format: PackageFormat::Flatpak,
            scope: "user".into(),
            description: "".into(),
            installed_size: None,
            reported_size: Some("150 MB".into()),
            installation: Some("default".into()),
            reference: Some("app/org.videolan.VLC/x86_64/beta".into()),
            path: None,
            is_executable: None,
            is_desktop_application: true,
            can_uninstall: true,
            unavailable_reason: "".into(),
        };

        let plan = manager
            .prepare(&installation, UninstallPlanOptions::default())
            .expect("prepare");
        assert_eq!(
            plan.target_path,
            Some("app/org.videolan.VLC/x86_64/beta".into())
        );
        assert!(!plan.is_blocked);
    }

    #[test]
    fn prepare_rejects_malicious_or_flag_identifiers() {
        let manager = TransactionManager {
            plans: Mutex::new(HashMap::new()),
            current_operation: Mutex::new(None),
        };

        let mut installation = ApplicationInstallation {
            id: "flatpak:user:default:--all".into(),
            name: "Malicious Flag".into(),
            native_id: "--all".into(),
            version: "1.0".into(),
            format: PackageFormat::Flatpak,
            scope: "user".into(),
            description: "".into(),
            installed_size: None,
            reported_size: None,
            installation: None,
            reference: None,
            path: None,
            is_executable: None,
            is_desktop_application: true,
            can_uninstall: true,
            unavailable_reason: "".into(),
        };

        // Flag as native_id must be rejected
        let result = manager.prepare(&installation, UninstallPlanOptions::default());
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, "INVALID_IDENTIFIER");

        // Malicious whitespace in native_id
        installation.native_id = "vlc && rm -rf /".into();
        let result2 = manager.prepare(&installation, UninstallPlanOptions::default());
        assert!(result2.is_err());
        assert_eq!(result2.unwrap_err().code, "INVALID_IDENTIFIER");

        // Flag in reference
        installation.native_id = "org.videolan.VLC".into();
        installation.reference = Some("--delete-data".into());
        let result3 = manager.prepare(&installation, UninstallPlanOptions::default());
        assert!(result3.is_err());
        assert_eq!(result3.unwrap_err().code, "INVALID_IDENTIFIER");
    }

    #[test]
    fn associated_desktop_entries_supports_bureau_and_user_dirs() {
        let _lock = crate::storage::TEST_STORAGE_LOCK.lock().unwrap();
        let temp_dir =
            std::env::temp_dir().join(format!("panel_test_bureau_{}", std::process::id()));
        let bureau_dir = temp_dir.join("Bureau");
        fs::create_dir_all(&bureau_dir).unwrap();

        let desktop_file = bureau_dir.join("french_app.desktop");
        let content =
            "[Desktop Entry]\nType=Application\nName=FrenchApp\nExec=/tmp/french_test.AppImage\n";
        fs::write(&desktop_file, content).unwrap();

        std::env::set_var("HOME", &temp_dir);

        let matches = find_associated_desktop_entries("/tmp/french_test.AppImage");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0], desktop_file);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn associated_desktop_entries_resolved_before_deletion_even_with_symlinks() {
        let _lock = crate::storage::TEST_STORAGE_LOCK.lock().unwrap();
        let temp_dir =
            std::env::temp_dir().join(format!("panel_test_symlink_del_{}", std::process::id()));
        let apps_dir = temp_dir.join("applications");
        fs::create_dir_all(&apps_dir).unwrap();

        // Real binary file in temp_dir
        let real_file = temp_dir.join("RealApp.AppImage");
        fs::write(&real_file, b"executable content").unwrap();

        // Symlink pointing to real binary
        let symlink_file = temp_dir.join("SymlinkApp.AppImage");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&real_file, &symlink_file).unwrap();

        // Desktop file pointing to the symlink!
        let desktop_file = apps_dir.join("symlink_app.desktop");
        let content = format!(
            "[Desktop Entry]\nType=Application\nName=SymlinkApp\nExec={}\n",
            symlink_file.display()
        );
        fs::write(&desktop_file, content).unwrap();

        std::env::set_var("XDG_DATA_HOME", &temp_dir);
        std::env::set_var("PANEL_CENTER_DATA_DIR", &temp_dir);
        std::env::set_var("PANEL_CENTER_CONFIG_DIR", &temp_dir);

        let manager = TransactionManager {
            plans: Mutex::new(HashMap::new()),
            current_operation: Mutex::new(None),
        };

        // Prepare uninstall targeting the real file
        let installation = ApplicationInstallation {
            id: format!("appimage:{}", real_file.display()),
            name: "Real App".into(),
            native_id: "real-app".into(),
            version: "1.0".into(),
            format: PackageFormat::Appimage,
            scope: "user".into(),
            description: "".into(),
            installed_size: Some(18),
            reported_size: Some("18 B".into()),
            installation: None,
            reference: None,
            path: Some(real_file.to_string_lossy().into()),
            is_executable: Some(true),
            is_desktop_application: true,
            can_uninstall: true,
            unavailable_reason: "".into(),
        };

        let plan = manager
            .prepare(&installation, UninstallPlanOptions::default())
            .expect("prepare succeeds");

        // The desktop file pointing to the symlink MUST be recognized as an affected item!
        assert!(plan
            .affected_items
            .iter()
            .any(|item| item.contains("symlink_app.desktop")));

        // Confirm uninstall
        let op = manager.confirm(&plan.id).expect("confirm succeeds");
        assert_eq!(op.status, OperationStatus::Completed);

        // Both the AppImage file AND the associated desktop entry must be deleted!
        assert!(!real_file.exists());
        assert!(!desktop_file.exists());

        let _ = fs::remove_dir_all(&temp_dir);
        std::env::remove_var("XDG_DATA_HOME");
        std::env::remove_var("PANEL_CENTER_DATA_DIR");
        std::env::remove_var("PANEL_CENTER_CONFIG_DIR");
    }

    #[test]
    fn associated_desktop_entries_supports_bin_env_and_flags() {
        let app_path = Path::new("/tmp/test_env_app.AppImage");

        // /bin/env wrapper
        let entry1 =
            "[Desktop Entry]\nType=Application\nName=App\nExec=/bin/env /tmp/test_env_app.AppImage\n";
        assert!(is_desktop_associated_with_appimage(entry1, app_path));

        // env with -u VAR flag
        let entry2 = "[Desktop Entry]\nType=Application\nName=App\nExec=env -u GDK_BACKEND /tmp/test_env_app.AppImage\n";
        assert!(is_desktop_associated_with_appimage(entry2, app_path));

        // env with multiple assignments and flags
        let entry3 = "[Desktop Entry]\nType=Application\nName=App\nExec=/usr/bin/env -u WAYLAND_DISPLAY FOO=BAR /tmp/test_env_app.AppImage --flag\n";
        assert!(is_desktop_associated_with_appimage(entry3, app_path));
    }

    #[test]
    fn associated_desktop_entries_canonical_deduplication() {
        let _lock = crate::storage::TEST_STORAGE_LOCK.lock().unwrap();
        let temp_dir =
            std::env::temp_dir().join(format!("panel_test_dedup_{}", std::process::id()));
        let bureau_dir = temp_dir.join("Bureau");
        fs::create_dir_all(&bureau_dir).unwrap();

        let desktop_file = bureau_dir.join("unique.desktop");
        let content =
            "[Desktop Entry]\nType=Application\nName=Unique\nExec=/tmp/unique_test.AppImage\n";
        fs::write(&desktop_file, content).unwrap();

        // Create symlink ~/Desktop -> ~/Bureau
        let desktop_dir = temp_dir.join("Desktop");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&bureau_dir, &desktop_dir).unwrap();

        std::env::set_var("HOME", &temp_dir);

        let matches = find_associated_desktop_entries("/tmp/unique_test.AppImage");
        // Must contain the file exactly once, no duplicates even though Desktop and Bureau point to same dir!
        assert_eq!(matches.len(), 1);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn execute_deb_removal_rejects_protected_package() {
        let manager = TransactionManager {
            plans: Mutex::new(HashMap::new()),
            current_operation: Mutex::new(None),
        };

        let plan = UninstallPlan {
            id: "plan_test_deb".into(),
            target_id: "deb:system:systemd:amd64".into(),
            target_name: "systemd".into(),
            format: PackageFormat::Deb,
            scope: "system".into(),
            native_id: "systemd".into(),
            target_path: None,
            target_installation: None,
            options: UninstallPlanOptions::default(),
            affected_items: vec![],
            deb_preview: None,
            data_impact: "".into(),
            estimated_freed_space: None,
            requires_auth: true,
            is_blocked: false, // simulate corrupted/tampered plan
            blocked_reason: None,
            created_at: 1000,
            expires_at: 2000,
        };

        let result = manager.execute_deb_removal(&plan);
        assert!(result.is_err());
        let (_, err) = result.unwrap_err();
        assert_eq!(err.code, "OPERATION_BLOCKED");
    }

    #[test]
    fn prevents_uninstalling_protected_snap() {
        let manager = TransactionManager {
            plans: Mutex::new(HashMap::new()),
            current_operation: Mutex::new(None),
        };

        let installation = ApplicationInstallation {
            id: "snap:system:snapd".into(),
            name: "snapd".into(),
            native_id: "snapd".into(),
            version: "2.66".into(),
            format: PackageFormat::Snap,
            scope: "system".into(),
            description: "".into(),
            installed_size: None,
            reported_size: None,
            installation: Some("system".into()),
            reference: Some("23545".into()),
            path: None,
            is_executable: None,
            is_desktop_application: false,
            can_uninstall: false,
            unavailable_reason: "".into(),
        };

        let plan = manager
            .prepare(&installation, UninstallPlanOptions::default())
            .expect("prepare returns blocked plan");
        assert!(plan.is_blocked);
        assert!(plan.requires_auth);

        let tampered = UninstallPlan {
            is_blocked: false,
            ..plan
        };
        let (_, err) = manager.execute_snap_removal(&tampered).unwrap_err();
        assert_eq!(err.code, "OPERATION_BLOCKED");
    }

    #[test]
    fn flatpak_system_named_installation_preserved_in_plan() {
        let manager = TransactionManager {
            plans: Mutex::new(HashMap::new()),
            current_operation: Mutex::new(None),
        };

        let installation = ApplicationInstallation {
            id: "flatpak:system:extra:app/org.videolan.VLC/x86_64/stable".into(),
            name: "VLC".into(),
            native_id: "org.videolan.VLC".into(),
            version: "3.0".into(),
            format: PackageFormat::Flatpak,
            scope: "system".into(),
            description: "".into(),
            installed_size: None,
            reported_size: Some("120 MB".into()),
            installation: Some("extra".into()),
            reference: Some("app/org.videolan.VLC/x86_64/stable".into()),
            path: None,
            is_executable: None,
            is_desktop_application: true,
            can_uninstall: true,
            unavailable_reason: "".into(),
        };

        let plan = manager
            .prepare(&installation, UninstallPlanOptions::default())
            .expect("prepare succeeds");
        assert!(plan.requires_auth);
        assert_eq!(plan.target_installation.as_deref(), Some("extra"));
        assert_eq!(
            plan.target_path.as_deref(),
            Some("app/org.videolan.VLC/x86_64/stable")
        );
    }

    #[test]
    fn detects_unexpected_extra_deb_packages_before_execution() {
        let plan = UninstallPlan {
            id: "plan_change_check".into(),
            target_id: "deb:system:vlc".into(),
            target_name: "VLC".into(),
            format: PackageFormat::Deb,
            scope: "system".into(),
            native_id: "vlc".into(),
            target_path: None,
            target_installation: None,
            options: UninstallPlanOptions::default(),
            affected_items: vec![
                "Paquet DEB : vlc".into(),
                "Dépendance retirée : vlc-bin".into(),
            ],
            deb_preview: None,
            data_impact: "".into(),
            estimated_freed_space: None,
            requires_auth: true,
            is_blocked: false,
            blocked_reason: None,
            created_at: 1000,
            expires_at: 2000,
        };
        assert!(!deb_plan_packages_changed(
            &plan,
            &["vlc".into(), "vlc-bin".into()],
            &[]
        ));
        assert!(deb_plan_packages_changed(
            &plan,
            &["vlc".into(), "vlc-bin".into(), "extra-pkg".into()],
            &[]
        ));
        assert!(deb_plan_packages_changed(
            &plan,
            &["vlc".into(), "vlc-bin".into()],
            &["unexpected-replacement".into()]
        ));
    }

    #[test]
    fn reconciles_interrupted_operation_without_inventing_success() {
        let _lock = crate::storage::TEST_STORAGE_LOCK.lock().unwrap();
        let temp_dir = std::env::temp_dir().join(format!(
            "panel_center_test_reconcile_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();
        std::env::set_var("PANEL_CENTER_DATA_DIR", &temp_dir);
        std::env::set_var("PANEL_CENTER_CONFIG_DIR", &temp_dir);

        let existing_file = temp_dir.join("still_here.AppImage");
        fs::write(&existing_file, b"appimage").unwrap();

        record_in_flight_operation(&InFlightOperation {
            operation_id: "op_interrupted_1".into(),
            plan_id: "plan_interrupted".into(),
            target_id: format!("appimage:{}", existing_file.display()),
            target_name: "InterruptedApp".into(),
            native_id: "interrupted-app".into(),
            format: PackageFormat::Appimage,
            scope: "user".into(),
            target_path: Some(existing_file.to_string_lossy().into()),
            target_installation: None,
            started_at: current_timestamp(),
        })
        .expect("record in-flight");

        let reconciled = reconcile_interrupted_operations();
        assert_eq!(reconciled, 1);
        assert!(list_in_flight_operations().is_empty());

        let history = crate::storage::load_history();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].status, OperationStatus::Failed);
        assert!(history[0].summary.contains("toujours présent"));

        let _ = fs::remove_dir_all(&temp_dir);
        std::env::remove_var("PANEL_CENTER_DATA_DIR");
        std::env::remove_var("PANEL_CENTER_CONFIG_DIR");
    }

    #[test]
    fn proc_locks_matches_only_active_dpkg_write_locks() {
        let targets = vec!["08:01:10224209".into(), "08:01:10224208".into()];
        let idle = "\
1: POSIX  ADVISORY  READ 19368 08:01:10224209 0 EOF
2: FLOCK  ADVISORY  WRITE 1780 08:01:9999999 0 EOF
";
        assert!(!proc_locks_has_write_lock(idle, &targets));

        let active = "\
1: POSIX  ADVISORY  READ 19368 08:01:5658061 128 128
2: POSIX  ADVISORY  WRITE 22451 08:01:10224209 0 EOF
";
        assert!(proc_locks_has_write_lock(active, &targets));
    }

    #[test]
    fn uninstalls_local_user_application_and_blocks_opt_system_install() {
        let _lock = crate::storage::TEST_STORAGE_LOCK.lock().unwrap();
        let prev_home = std::env::var_os("HOME");
        let temp_home = std::env::temp_dir().join(format!(
            "panel_center_test_local_uninstall_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&temp_home);
        let app_root = temp_home.join(".local/ABDownloadManager");
        let app_bin = app_root.join("bin/ABDownloadManager");
        let apps_dir = temp_home.join(".local/share/applications");
        fs::create_dir_all(app_bin.parent().unwrap()).unwrap();
        fs::create_dir_all(&apps_dir).unwrap();
        fs::write(&app_bin, b"binary").unwrap();
        let launcher = apps_dir.join("com.abdownloadmanager.desktop");
        fs::write(
            &launcher,
            b"[Desktop Entry]\nType=Application\nName=AB Download Manager\n",
        )
        .unwrap();

        std::env::set_var("HOME", &temp_home);
        std::env::set_var("PANEL_CENTER_DATA_DIR", &temp_home);
        std::env::set_var("PANEL_CENTER_CONFIG_DIR", &temp_home);

        let manager = TransactionManager {
            plans: Mutex::new(HashMap::new()),
            current_operation: Mutex::new(None),
        };

        // 1. User-owned local app uninstall succeeds and removes both folder and .desktop
        let user_app = ApplicationInstallation {
            id: format!("local:user:{}", launcher.display()),
            name: "AB Download Manager".into(),
            native_id: "com.abdownloadmanager".into(),
            version: "".into(),
            format: PackageFormat::Local,
            scope: "user".into(),
            description: "Downloader".into(),
            installed_size: Some(6),
            reported_size: None,
            installation: Some(app_root.to_string_lossy().into()),
            reference: Some(launcher.to_string_lossy().into()),
            path: Some(app_bin.to_string_lossy().into()),
            is_executable: Some(true),
            is_desktop_application: true,
            can_uninstall: true,
            unavailable_reason: "".into(),
        };

        let plan = manager
            .prepare(&user_app, UninstallPlanOptions::default())
            .expect("prepare local user app");
        assert!(!plan.is_blocked);
        assert!(!plan.requires_auth);

        let op = manager.confirm(&plan.id).expect("confirm local uninstall");
        assert_eq!(op.status, OperationStatus::Completed);
        assert!(!app_root.exists());
        assert!(!launcher.exists());

        // 2. System /opt local app is blocked for safety
        let opt_app = ApplicationInstallation {
            id: "local:system:/home/test/.local/share/applications/antigravity.desktop".into(),
            name: "Antigravity".into(),
            native_id: "antigravity".into(),
            version: "".into(),
            format: PackageFormat::Local,
            scope: "system".into(),
            description: "".into(),
            installed_size: None,
            reported_size: None,
            installation: Some("/opt/Antigravity".into()),
            reference: None,
            path: Some("/opt/Antigravity/antigravity".into()),
            is_executable: Some(true),
            is_desktop_application: true,
            can_uninstall: false,
            unavailable_reason: "".into(),
        };

        let blocked_plan = manager
            .prepare(&opt_app, UninstallPlanOptions::default())
            .expect("prepare opt app");
        assert!(blocked_plan.is_blocked);

        // 3. Shared directory like ~/.local/bin must be rejected by validator
        assert!(validate_safe_local_install_dir(&temp_home.join(".local/bin")).is_err());
        assert!(validate_safe_local_install_dir(&temp_home.join(".local")).is_err());
        assert!(validate_safe_local_install_dir(&temp_home).is_err());

        let _ = fs::remove_dir_all(&temp_home);
        if let Some(h) = prev_home {
            std::env::set_var("HOME", h);
        }
        std::env::remove_var("PANEL_CENTER_DATA_DIR");
        std::env::remove_var("PANEL_CENTER_CONFIG_DIR");
    }
}
