use crate::domain::{
    protection::is_protected_deb, AppError, ApplicationInstallation, HistoryEntry, Operation,
    OperationStatus, PackageFormat, UninstallPlan, UninstallPlanOptions,
};
use crate::storage::append_history;
use std::collections::HashMap;
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

pub fn is_dpkg_locked() -> bool {
    if let Ok(entries) = fs::read_dir("/proc") {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.chars().all(|c| c.is_ascii_digit()) {
                    let comm_path = path.join("comm");
                    if let Ok(comm) = fs::read_to_string(comm_path) {
                        let proc_name = comm.trim();
                        if matches!(
                            proc_name,
                            "apt"
                                | "apt-get"
                                | "dpkg"
                                | "dpkg-deb"
                                | "unattended-upgr"
                                | "aptitude"
                                | "synaptic"
                                | "packagekitd"
                                | "apt-fast"
                        ) {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
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
        if installation.native_id.starts_with('-')
            || installation
                .native_id
                .contains(|c: char| c.is_whitespace() || c == '\0')
        {
            return Err(AppError::new(
                "INVALID_IDENTIFIER",
                "L’identifiant de l’application contient des caractères non autorisés.",
            ));
        }

        if let Some(ref r) = installation.reference {
            if r.starts_with('-') || r.contains(|c: char| c.is_whitespace() || c == '\0') {
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
            },
            installation.native_id.replace(['/', ':', ' '], "_"),
            now,
            counter
        );

        let (requires_auth, affected_items, data_impact, is_blocked, blocked_reason) =
            match installation.format {
                PackageFormat::Deb => {
                    let items = vec![format!("Paquet DEB : {}", installation.native_id)];
                    let (blocked, reason) = if is_protected_deb(&installation.native_id) {
                        (
                            true,
                            Some(
                                "Ce paquet est essentiel au fonctionnement du système Ubuntu et ne peut pas être supprimé."
                                    .into(),
                            ),
                        )
                    } else if is_dpkg_locked() {
                        (
                            true,
                            Some(
                                "Le gestionnaire de paquets APT/dpkg est actuellement verrouillé par une autre opération système. Veuillez réessayer ultérieurement."
                                    .into(),
                            ),
                        )
                    } else {
                        (false, None)
                    };
                    let impact = if options.purge_config {
                        "Le paquet sera supprimé avec ses fichiers de configuration système (/etc). Les fichiers personnels ne sont pas affectés."
                            .into()
                    } else {
                        "Le paquet sera retiré. Ses fichiers de configuration système sont conservés pour une réinstallation ultérieure."
                            .into()
                    };
                    (true, items, impact, blocked, reason)
                }
                PackageFormat::Snap => {
                    let items = vec![format!("Snap : {}", installation.native_id)];
                    let impact =
                        "Le paquet Snap sera supprimé. Un instantané (snapshot) des données est automatiquement sauvegardé par snapd."
                            .into();
                    (true, items, impact, false, None)
                }
                PackageFormat::Flatpak => {
                    let ref_label = installation
                        .reference
                        .clone()
                        .unwrap_or_else(|| installation.native_id.clone());
                    let items = vec![format!("Flatpak ({}) : {}", installation.scope, ref_label)];
                    let impact = if options.remove_data {
                        "L’application Flatpak et ses données utilisateur stockées dans ~/.var/app seront supprimées."
                            .into()
                    } else {
                        "L’application Flatpak sera désinstallée. Les données utilisateur dans ~/.var/app restent intactes."
                            .into()
                    };
                    (installation.scope == "system", items, impact, false, None)
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
            };

        let target_path = match installation.format {
            PackageFormat::Appimage => installation.path.clone(),
            PackageFormat::Flatpak => installation
                .reference
                .clone()
                .or_else(|| Some(installation.native_id.clone())),
            _ => None,
        };

        let plan = UninstallPlan {
            id: plan_id.clone(),
            target_id: installation.id.clone(),
            target_name: installation.name.clone(),
            format: installation.format,
            scope: installation.scope.clone(),
            native_id: installation.native_id.clone(),
            target_path,
            options,
            affected_items,
            data_impact,
            estimated_freed_space: installation
                .reported_size
                .clone()
                .or_else(|| installation.installed_size.map(|s| format!("{} octets", s))),
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

        // 4. Execute operation
        let exec_result = self.execute_plan(&plan);

        // 5. Update operation state
        let finish_time = current_timestamp();
        let (final_status, final_msg, app_err) = match exec_result {
            Ok(msg) => (OperationStatus::Completed, msg, None),
            Err(err) => (
                OperationStatus::Failed,
                format!("Échec de la désinstallation : {}", err.message),
                Some(err),
            ),
        };

        operation.status = final_status;
        operation.step = if final_status == OperationStatus::Completed {
            "Terminé".into()
        } else {
            "Échec".into()
        };
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

        if let Some(err) = app_err {
            Err(err)
        } else {
            Ok(operation)
        }
    }

    fn execute_plan(&self, plan: &UninstallPlan) -> Result<String, AppError> {
        match plan.format {
            PackageFormat::Appimage => self.execute_appimage_removal(plan),
            PackageFormat::Flatpak => self.execute_flatpak_removal(plan),
            PackageFormat::Deb => self.execute_deb_removal(plan),
            PackageFormat::Snap => self.execute_snap_removal(plan),
        }
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

    fn execute_flatpak_removal(&self, plan: &UninstallPlan) -> Result<String, AppError> {
        if plan.scope == "user" {
            let target_ref = plan
                .target_path
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or(&plan.native_id);

            // Validation against flag injection and malformed identifiers
            if target_ref.starts_with('-')
                || target_ref.contains(|c: char| c.is_whitespace() || c == '\0')
            {
                return Err(AppError::new(
                    "INVALID_IDENTIFIER",
                    "L’identifiant Flatpak contient des caractères non autorisés.",
                ));
            }

            let mut args = vec!["uninstall", "--user", "-y", "--noninteractive"];
            if plan.options.remove_data {
                args.push("--delete-data");
            }
            args.push("--");
            args.push(target_ref);

            let flatpak_bin = if Path::new("/usr/bin/flatpak").exists() {
                "/usr/bin/flatpak"
            } else {
                "flatpak"
            };

            let output = std::process::Command::new(flatpak_bin)
                .args(&args)
                .output()
                .map_err(|e| {
                    AppError::new(
                        "FLATPAK_EXEC_FAILED",
                        &format!("Impossible d’exécuter la commande Flatpak : {e}"),
                    )
                })?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(AppError::new(
                    "FLATPAK_UNINSTALL_ERROR",
                    &format!("Erreur lors de la désinstallation Flatpak : {stderr}"),
                ));
            }

            // Post-operation verification: check that application is no longer detected
            let check_output = std::process::Command::new(flatpak_bin)
                .args(["info", "--user", "--", target_ref])
                .output();

            if let Ok(check) = check_output {
                if check.status.success() {
                    return Err(AppError::new(
                        "FLATPAK_VERIFY_FAILED",
                        "L’application Flatpak est toujours détectée sur le système après la désinstallation.",
                    ));
                }
            }

            Ok(format!(
                "L’application Flatpak {} a été retirée avec succès.",
                plan.target_name
            ))
        } else {
            // System flatpak requires Polkit privileged helper
            Err(AppError::new(
                "AUTH_SERVICE_REQUIRED",
                "La désinstallation d’une application Flatpak système requiert une autorisation administrateur Polkit.",
            ))
        }
    }

    fn execute_deb_removal(&self, plan: &UninstallPlan) -> Result<String, AppError> {
        if is_protected_deb(&plan.native_id) {
            return Err(AppError::new(
                "OPERATION_BLOCKED",
                "Ce paquet est essentiel au fonctionnement du système Ubuntu et ne peut pas être supprimé.",
            ));
        }

        if is_dpkg_locked() {
            return Err(AppError::new(
                "MANAGER_LOCKED",
                "Le gestionnaire de paquets APT/dpkg est actuellement verrouillé par une autre opération système. Veuillez réessayer ultérieurement.",
            ));
        }

        // In development mode, system package removal requires the privileged D-Bus service
        Err(AppError::new(
            "AUTH_SERVICE_REQUIRED",
            "La désinstallation d’un paquet DEB système requiert le service privilégié Polkit (environnement système avec paquet installé).",
        ))
    }

    fn execute_snap_removal(&self, _plan: &UninstallPlan) -> Result<String, AppError> {
        Err(AppError::new(
            "AUTH_SERVICE_REQUIRED",
            "La suppression d’un paquet Snap requiert une autorisation administrateur Polkit.",
        ))
    }

    pub fn get_current_operation(&self) -> Option<Operation> {
        self.current_operation.lock().unwrap().clone()
    }
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
            options: UninstallPlanOptions::default(),
            affected_items: vec![],
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
        assert_eq!(result.unwrap_err().code, "OPERATION_BLOCKED");
    }
}
