use crate::domain::{
    protection::{is_protected_deb, is_protected_snap},
    AppError, DebAffectedPackage, DebDependencyPreview, OperationStatus,
};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::{Command, Stdio};

pub const PKEXEC_BIN: &str = "/usr/bin/pkexec";
pub const APT_GET_BIN: &str = "/usr/bin/apt-get";
pub const DPKG_QUERY_BIN: &str = "/usr/bin/dpkg-query";
pub const SNAP_BIN: &str = "/usr/bin/snap";
pub const FLATPAK_BIN: &str = "/usr/bin/flatpak";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebSimulation {
    pub is_installed: bool,
    pub is_essential_or_protected: bool,
    pub packages_to_remove: Vec<String>,
    pub protected_in_cascade: Vec<String>,
    pub preview: DebDependencyPreview,
}

#[derive(Debug, Clone)]
struct DebPackageMetadata {
    version: String,
    installed_size: Option<u64>,
    is_essential_or_protected: bool,
    description: String,
}

/// Validates a Debian package identifier according to Debian Policy §5.6.1,
/// with optional `:arch` suffix. Rejects shell metacharacters, wildcards,
/// flags, and whitespace.
pub fn validate_deb_package_name(name: &str) -> Result<(), AppError> {
    let mut parts = name.split(':');
    let pkg = parts.next().unwrap_or("");
    let arch = parts.next();
    if parts.next().is_some() {
        return Err(AppError::new(
            "INVALID_IDENTIFIER",
            "L’identifiant du paquet DEB contient un format d’architecture invalide.",
        ));
    }

    let valid_pkg = (2..=128).contains(&pkg.len())
        && pkg
            .as_bytes()
            .first()
            .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        && pkg.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'+' | b'.' | b'-')
        });

    let valid_arch = arch.is_none_or(|a| {
        (1..=32).contains(&a.len())
            && a.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            && !a.starts_with('-')
    });

    if !valid_pkg || !valid_arch {
        return Err(AppError::new(
            "INVALID_IDENTIFIER",
            "L’identifiant du paquet DEB contient des caractères non autorisés.",
        ));
    }
    Ok(())
}

/// Validates a Snap package name (optionally with `_instance` parallel suffix).
pub fn validate_snap_name(name: &str) -> Result<(), AppError> {
    let mut parts = name.split('_');
    let base = parts.next().unwrap_or("");
    let instance = parts.next();
    if parts.next().is_some() {
        return Err(AppError::new(
            "INVALID_IDENTIFIER",
            "L’identifiant Snap contient des caractères non autorisés.",
        ));
    }

    let valid_base = (1..=40).contains(&base.len())
        && !base.starts_with('-')
        && !base.ends_with('-')
        && !base.contains("--")
        && base
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');

    let valid_instance = instance.is_none_or(|inst| {
        (1..=10).contains(&inst.len())
            && inst
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    });

    if !valid_base || !valid_instance {
        return Err(AppError::new(
            "INVALID_IDENTIFIER",
            "L’identifiant Snap contient des caractères non autorisés.",
        ));
    }
    Ok(())
}

/// Validates a Flatpak application ID or full reference (`app/<id>/<arch>/<branch>`).
pub fn validate_flatpak_ref(reference: &str) -> Result<(), AppError> {
    let valid = !reference.is_empty()
        && reference.len() <= 256
        && !reference.starts_with('-')
        && !reference.contains("..")
        && reference
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'/'));

    if !valid {
        return Err(AppError::new(
            "INVALID_IDENTIFIER",
            "La référence Flatpak contient des caractères non autorisés.",
        ));
    }
    Ok(())
}

/// Validates a named Flatpak system installation identifier (e.g., `extra`).
pub fn validate_flatpak_installation(installation: &str) -> Result<(), AppError> {
    let valid = !installation.is_empty()
        && installation.len() <= 64
        && !installation.starts_with('-')
        && installation
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));

    if !valid {
        return Err(AppError::new(
            "INVALID_IDENTIFIER",
            "Le nom d’installation Flatpak contient des caractères non autorisés.",
        ));
    }
    Ok(())
}

/// Returns the exact Flatpak CLI installation scope flag for a system installation.
pub fn flatpak_system_scope_flag(installation: Option<&str>) -> Result<String, AppError> {
    match installation.map(str::trim).filter(|s| !s.is_empty()) {
        None | Some("default") | Some("system") => Ok("--system".into()),
        Some(named) => {
            validate_flatpak_installation(named)?;
            Ok(format!("--installation={named}"))
        }
    }
}

/// Parses language-independent `Remv ` and `Purg ` lines from `LC_ALL=C apt-get --simulate`,
/// returning `(package_name, version_if_present, is_purge)` in deterministic order.
pub fn parse_apt_simulate_removals_detailed(stdout: &str) -> Vec<(String, String, bool)> {
    let mut packages = Vec::new();
    let mut seen = HashSet::new();
    for line in stdout.lines() {
        let (rest, is_purge) = if let Some(r) = line.strip_prefix("Purg ") {
            (r, true)
        } else if let Some(r) = line.strip_prefix("Remv ") {
            (r, false)
        } else {
            continue;
        };

        let mut tokens = rest.split_whitespace();
        let Some(pkg) = tokens.next().filter(|p| !p.is_empty()) else {
            continue;
        };
        if !seen.insert(pkg.to_string()) {
            continue;
        }
        let version = tokens
            .next()
            .and_then(|tok| tok.strip_prefix('['))
            .map(|tok| tok.trim_end_matches(']').to_string())
            .unwrap_or_default();
        packages.push((pkg.to_string(), version, is_purge));
    }
    packages
}

/// Convenience wrapper returning only the deduplicated package names from `Remv `/`Purg ` lines.
pub fn parse_apt_simulate_removals(stdout: &str) -> Vec<String> {
    parse_apt_simulate_removals_detailed(stdout)
        .into_iter()
        .map(|(pkg, _, _)| pkg)
        .collect()
}

/// Parses replacement or newly installed packages (`Inst <pkg> ...`) reported by `apt-get --simulate`.
pub fn parse_apt_simulate_installs(stdout: &str) -> Vec<String> {
    let mut packages = Vec::new();
    let mut seen = HashSet::new();
    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix("Inst ") {
            if let Some(pkg) = rest.split_whitespace().next().filter(|p| !p.is_empty()) {
                if seen.insert(pkg.to_string()) {
                    packages.push(pkg.to_string());
                }
            }
        }
    }
    packages
}

/// Parses automatically installed packages no longer required (autoremove candidates)
/// from `LC_ALL=C apt-get --simulate`.
pub fn parse_apt_autoremove_candidates(stdout: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    let mut seen = HashSet::new();
    let mut in_section = false;

    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("The following package was automatically installed")
            || trimmed.starts_with("The following packages were automatically installed")
        {
            in_section = true;
            continue;
        }
        if in_section {
            if line.starts_with(' ') || line.starts_with('\t') {
                for tok in trimmed.split_whitespace() {
                    if !tok.is_empty() && seen.insert(tok.to_string()) {
                        candidates.push(tok.to_string());
                    }
                }
            } else {
                in_section = false;
            }
        }
    }
    candidates
}

/// Parses blocking reverse-dependent package names from APT's
/// `"The following packages have unmet dependencies:"` diagnostic block.
pub fn parse_apt_unmet_dependents(output: &str) -> Vec<String> {
    let mut dependents = Vec::new();
    let mut seen = HashSet::new();
    let mut in_section = false;

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("The following packages have unmet dependencies:") {
            in_section = true;
            continue;
        }
        if in_section {
            if line.starts_with(' ') || line.starts_with('\t') {
                if let Some((pkg_part, _)) = trimmed.split_once(" : ") {
                    let pkg = pkg_part.trim();
                    if !pkg.is_empty() && seen.insert(pkg.to_string()) {
                        dependents.push(pkg.to_string());
                    }
                }
            } else if !trimmed.is_empty() {
                in_section = false;
            }
        }
    }
    dependents
}

/// Formats byte sizes in human-readable French units (aligned with frontend `formatSize`).
pub fn format_bytes_fr(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["octets", "Ko", "Mo", "Go"];
    let mut value = bytes as f64;
    let mut unit_idx = 0;
    while value >= 1024.0 && unit_idx < UNITS.len() - 1 {
        value /= 1024.0;
        unit_idx += 1;
    }
    if unit_idx == 0 {
        format!("{bytes} {}", UNITS[0])
    } else if value >= 10.0 {
        format!("{value:.0} {}", UNITS[unit_idx])
    } else {
        let formatted = format!("{value:.1}").replace('.', ",");
        format!("{formatted} {}", UNITS[unit_idx])
    }
}

/// Queries dpkg for a single package's `(is_installed, is_essential_or_protected)` status.
pub fn query_deb_status(package: &str) -> Result<(bool, bool), AppError> {
    validate_deb_package_name(package)?;
    if !Path::new(DPKG_QUERY_BIN).exists() {
        return Err(AppError::new(
            "MANAGER_UNAVAILABLE",
            "Le gestionnaire dpkg est indisponible sur ce système.",
        ));
    }

    let output = Command::new(DPKG_QUERY_BIN)
        .args([
            "--show",
            "--showformat=${Essential}\t${Protected}\t${db:Status-Abbrev}\n",
            "--",
            package,
        ])
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .output()
        .map_err(|_| {
            AppError::new(
                "DEB_QUERY_FAILED",
                "Impossible de vérifier l’état du paquet DEB.",
            )
        })?;

    if !output.status.success() {
        return Ok((false, false));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let cols: Vec<_> = line.split('\t').collect();
        if cols.len() >= 3 {
            let essential = cols[0].trim().eq_ignore_ascii_case("yes");
            let protected = cols[1].trim().eq_ignore_ascii_case("yes");
            let installed = cols[2].as_bytes().get(1) == Some(&b'i');
            if installed {
                return Ok((true, essential || protected));
            }
        }
    }
    Ok((false, false))
}

/// Batch-queries dpkg metadata for multiple packages in a single subprocess call.
fn batch_query_deb_metadata(packages: &[String]) -> HashMap<String, DebPackageMetadata> {
    let mut map = HashMap::new();
    if packages.is_empty() || !Path::new(DPKG_QUERY_BIN).exists() {
        return map;
    }

    let mut args = vec![
        "--show".to_string(),
        "--showformat=${binary:Package}\t${Version}\t${Installed-Size}\t${Essential}\t${Protected}\t${db:Status-Abbrev}\t${binary:Synopsis}\n".to_string(),
        "--".to_string(),
    ];
    for pkg in packages {
        if validate_deb_package_name(pkg).is_ok() {
            args.push(pkg.clone());
        }
    }
    if args.len() <= 3 {
        return map;
    }

    let Ok(output) = Command::new(DPKG_QUERY_BIN)
        .args(&args)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .output()
    else {
        return map;
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let cols: Vec<_> = line.splitn(7, '\t').collect();
        if cols.len() != 7 || cols[5].as_bytes().get(1) != Some(&b'i') {
            continue;
        }
        let pkg_key = cols[0].trim().to_string();
        let meta = DebPackageMetadata {
            version: cols[1].trim().to_string(),
            installed_size: cols[2]
                .trim()
                .parse::<u64>()
                .ok()
                .and_then(|kb| kb.checked_mul(1024)),
            is_essential_or_protected: cols[3].trim().eq_ignore_ascii_case("yes")
                || cols[4].trim().eq_ignore_ascii_case("yes"),
            description: cols[6].trim().to_string(),
        };
        let bare = pkg_key.split(':').next().unwrap_or(&pkg_key).to_string();
        map.entry(bare).or_insert_with(|| meta.clone());
        map.insert(pkg_key, meta);
    }
    map
}

fn is_same_deb_package(a: &str, b: &str) -> bool {
    a == b || a.split(':').next() == b.split(':').next()
}

/// Runs a non-privileged, read-only APT simulation (`apt-get --simulate`) to discover
/// the full dependency impact: target package, cascade reverse dependencies, replacement
/// packages, unused autoremove candidates, blocking dependents, and total freed size.
pub fn simulate_deb_removal(package: &str, purge_config: bool) -> Result<DebSimulation, AppError> {
    validate_deb_package_name(package)?;
    let (is_installed, is_essential_or_protected) = query_deb_status(package)?;
    if !is_installed {
        return Ok(DebSimulation {
            is_installed: false,
            is_essential_or_protected,
            packages_to_remove: Vec::new(),
            protected_in_cascade: Vec::new(),
            preview: DebDependencyPreview {
                target_package: package.to_string(),
                removed_packages: Vec::new(),
                dependent_packages: Vec::new(),
                newly_installed_packages: Vec::new(),
                autoremove_candidates: Vec::new(),
                blocking_dependents: Vec::new(),
                total_freed_bytes: None,
            },
        });
    }

    if !Path::new(APT_GET_BIN).exists() {
        return Err(AppError::new(
            "MANAGER_UNAVAILABLE",
            "Le gestionnaire APT est indisponible sur ce système.",
        ));
    }

    let action = if purge_config { "purge" } else { "remove" };
    let output = Command::new(APT_GET_BIN)
        .args([
            "--simulate",
            "--no-install-recommends",
            action,
            "--",
            package,
        ])
        .env("LC_ALL", "C")
        .env("DEBIAN_FRONTEND", "noninteractive")
        .stdin(Stdio::null())
        .output()
        .map_err(|_| {
            AppError::new(
                "APT_SIMULATE_FAILED",
                "Impossible de simuler la désinstallation avec APT.",
            )
        })?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}\n{stderr}");

    if !output.status.success() {
        let blocking_dependents = parse_apt_unmet_dependents(&combined);
        let protected_blockers: Vec<String> = blocking_dependents
            .iter()
            .filter(|pkg| is_protected_deb(pkg))
            .cloned()
            .collect();

        if stderr.contains("essential")
            || stdout.contains("essential")
            || !blocking_dependents.is_empty()
        {
            let mut protected_in_cascade = protected_blockers;
            if (stderr.contains("essential") || stdout.contains("essential"))
                && !protected_in_cascade.iter().any(|p| p == package)
            {
                protected_in_cascade.push(package.to_string());
            }
            return Ok(DebSimulation {
                is_installed: true,
                is_essential_or_protected: is_essential_or_protected
                    || stderr.contains("essential")
                    || stdout.contains("essential"),
                packages_to_remove: vec![package.to_string()],
                protected_in_cascade,
                preview: DebDependencyPreview {
                    target_package: package.to_string(),
                    removed_packages: Vec::new(),
                    dependent_packages: Vec::new(),
                    newly_installed_packages: Vec::new(),
                    autoremove_candidates: Vec::new(),
                    blocking_dependents,
                    total_freed_bytes: None,
                },
            });
        }

        return Err(AppError::new(
            "APT_SIMULATE_FAILED",
            &format!(
                "APT n’a pas pu calculer l’aperçu de désinstallation : {}",
                sanitize_stderr(&stderr)
            ),
        ));
    }

    let mut detailed = parse_apt_simulate_removals_detailed(&stdout);
    if detailed.is_empty() {
        detailed.push((package.to_string(), String::new(), purge_config));
    }
    let packages_to_remove: Vec<String> = detailed.iter().map(|(p, _, _)| p.clone()).collect();
    let newly_installed_packages = parse_apt_simulate_installs(&stdout);
    let autoremove_candidates = parse_apt_autoremove_candidates(&stdout);

    // Enrich all affected packages in a single batch dpkg-query call
    let metadata_map = batch_query_deb_metadata(&packages_to_remove);

    let mut removed_packages = Vec::with_capacity(detailed.len());
    let mut dependent_packages = Vec::new();
    let mut protected_in_cascade = Vec::new();
    let mut total_freed: u64 = 0;
    let mut has_size = false;

    for (pkg_name, sim_version, is_purge) in detailed {
        let meta = metadata_map
            .get(&pkg_name)
            .or_else(|| metadata_map.get(pkg_name.split(':').next().unwrap_or(&pkg_name)));
        let is_prot =
            is_protected_deb(&pkg_name) || meta.is_some_and(|m| m.is_essential_or_protected);
        if is_prot {
            protected_in_cascade.push(pkg_name.clone());
        }

        let installed_size = meta.and_then(|m| m.installed_size);
        if let Some(size) = installed_size {
            has_size = true;
            total_freed = total_freed.saturating_add(size);
        }

        let is_target = is_same_deb_package(&pkg_name, package);
        let affected = DebAffectedPackage {
            name: pkg_name,
            version: if !sim_version.is_empty() {
                sim_version
            } else {
                meta.map(|m| m.version.clone()).unwrap_or_default()
            },
            installed_size,
            description: meta.map(|m| m.description.clone()).unwrap_or_default(),
            is_target,
            is_protected: is_prot,
            action: if is_purge { "purge" } else { "remove" }.into(),
        };
        if !is_target {
            dependent_packages.push(affected.clone());
        }
        removed_packages.push(affected);
    }

    // Place target package first for readable presentation
    removed_packages.sort_by_key(|p| (!p.is_target, p.name.clone()));
    dependent_packages.sort_by(|a, b| a.name.cmp(&b.name));

    let preview = DebDependencyPreview {
        target_package: package.to_string(),
        removed_packages,
        dependent_packages,
        newly_installed_packages,
        autoremove_candidates,
        blocking_dependents: Vec::new(),
        total_freed_bytes: has_size.then_some(total_freed),
    };

    Ok(DebSimulation {
        is_installed: true,
        is_essential_or_protected,
        packages_to_remove,
        protected_in_cascade,
        preview,
    })
}

/// Builds the exact `pkexec` argument vector for removing a DEB package via APT
/// (never enabling `--auto-remove`).
pub fn build_deb_pkexec_args(package: &str, purge_config: bool) -> Result<Vec<String>, AppError> {
    validate_deb_package_name(package)?;
    if is_protected_deb(package) {
        return Err(AppError::new(
            "OPERATION_BLOCKED",
            "Ce paquet est essentiel au fonctionnement du système Ubuntu et ne peut pas être supprimé.",
        ));
    }
    let action = if purge_config { "purge" } else { "remove" };
    Ok(vec![
        APT_GET_BIN.into(),
        "-y".into(),
        "-q".into(),
        "-o".into(),
        "DPkg::Lock::Timeout=0".into(),
        action.into(),
        "--".into(),
        package.into(),
    ])
}

/// Builds the exact `pkexec` argument vector for removing a Snap package.
pub fn build_snap_pkexec_args(snap_name: &str) -> Result<Vec<String>, AppError> {
    validate_snap_name(snap_name)?;
    if is_protected_snap(snap_name) {
        return Err(AppError::new(
            "OPERATION_BLOCKED",
            "Ce composant Snap système est protégé et ne peut pas être retiré.",
        ));
    }
    Ok(vec![
        SNAP_BIN.into(),
        "remove".into(),
        "--".into(),
        snap_name.into(),
    ])
}

/// Builds the exact `pkexec` argument vector for removing a system or named Flatpak app.
pub fn build_flatpak_system_pkexec_args(
    target_ref: &str,
    installation: Option<&str>,
    remove_data: bool,
) -> Result<(Vec<String>, String), AppError> {
    validate_flatpak_ref(target_ref)?;
    let scope_flag = flatpak_system_scope_flag(installation)?;
    let mut args = vec![
        FLATPAK_BIN.into(),
        "uninstall".into(),
        scope_flag.clone(),
        "-y".into(),
        "--noninteractive".into(),
    ];
    if remove_data {
        args.push("--delete-data".into());
    }
    args.push("--".into());
    args.push(target_ref.into());
    Ok((args, scope_flag))
}

/// Maps `pkexec` exit codes to explicit domain outcomes (`Cancelled` vs `Failed`).
/// Per `pkexec(1)`: exit code 126 = user dismissed the auth dialog, 127 = auth failed/denied.
pub fn classify_pkexec_exit(
    exit_code: Option<i32>,
    stderr: &str,
    manager_label: &str,
) -> Result<(), (OperationStatus, AppError)> {
    match exit_code {
        Some(0) => Ok(()),
        Some(126) => Err((
            OperationStatus::Cancelled,
            AppError::new(
                "AUTH_CANCELLED",
                "Autorisation administrateur annulée. Aucune modification n’a été effectuée.",
            ),
        )),
        Some(127) => Err((
            OperationStatus::Failed,
            AppError::new(
                "AUTH_DENIED",
                "Autorisation administrateur refusée ou impossible d’obtenir les privilèges Polkit. Aucune modification n’a été effectuée.",
            ),
        )),
        _ => {
            let lower = stderr.to_lowercase();
            if lower.contains("could not get lock")
                || lower.contains("unable to acquire the dpkg frontend lock")
                || lower.contains("another change")
            {
                return Err((
                    OperationStatus::Failed,
                    AppError::new(
                        "MANAGER_LOCKED",
                        &format!(
                            "Le gestionnaire {manager_label} est verrouillé par une autre opération. Réessayez dans quelques instants."
                        ),
                    ),
                ));
            }
            let detail = sanitize_stderr(stderr);
            let message = if detail.is_empty() {
                format!("La désinstallation {manager_label} a échoué.")
            } else {
                format!("Erreur lors de la désinstallation {manager_label} : {detail}")
            };
            Err((
                OperationStatus::Failed,
                AppError::new("PRIVILEGED_UNINSTALL_FAILED", &message),
            ))
        }
    }
}

/// Executes a validated privileged operation via Polkit (`pkexec`).
pub fn run_pkexec(args: &[String], manager_label: &str) -> Result<(), (OperationStatus, AppError)> {
    if !Path::new(PKEXEC_BIN).exists() {
        return Err((
            OperationStatus::Failed,
            AppError::new(
                "PKEXEC_UNAVAILABLE",
                "Polkit (pkexec) est indisponible sur ce système. Impossible de demander les droits administrateur.",
            ),
        ));
    }

    let output = Command::new(PKEXEC_BIN)
        .args(args)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| {
            (
                OperationStatus::Failed,
                AppError::new(
                    "PKEXEC_EXEC_FAILED",
                    &format!("Impossible de lancer l’autorisation Polkit : {e}"),
                ),
            )
        })?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    classify_pkexec_exit(output.status.code(), &stderr, manager_label)
}

/// Verifies after removal that a DEB package is no longer installed.
pub fn verify_deb_removed(package: &str) -> Result<(), AppError> {
    let (is_installed, _) = query_deb_status(package)?;
    if is_installed {
        return Err(AppError::new(
            "DEB_VERIFY_FAILED",
            "Le paquet DEB est toujours détecté comme installé après la tentative de suppression.",
        ));
    }
    Ok(())
}

/// Verifies after removal that a Snap package is no longer listed.
pub fn verify_snap_removed(snap_name: &str) -> Result<(), AppError> {
    validate_snap_name(snap_name)?;
    if !Path::new(SNAP_BIN).exists() {
        return Ok(());
    }
    let output = Command::new(SNAP_BIN)
        .args(["list", "--", snap_name])
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .output();
    if let Ok(check) = output {
        if check.status.success() {
            return Err(AppError::new(
                "SNAP_VERIFY_FAILED",
                "Le paquet Snap est toujours détecté sur le système après la désinstallation.",
            ));
        }
    }
    Ok(())
}

/// Verifies after removal that a system or named Flatpak app is no longer installed.
pub fn verify_flatpak_removed(target_ref: &str, scope_flag: &str) -> Result<(), AppError> {
    validate_flatpak_ref(target_ref)?;
    if !Path::new(FLATPAK_BIN).exists() {
        return Ok(());
    }
    let output = Command::new(FLATPAK_BIN)
        .args(["info", scope_flag, "--", target_ref])
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .output();
    if let Ok(check) = output {
        if check.status.success() {
            return Err(AppError::new(
                "FLATPAK_VERIFY_FAILED",
                "L’application Flatpak est toujours détectée sur le système après la désinstallation.",
            ));
        }
    }
    Ok(())
}

fn sanitize_stderr(stderr: &str) -> String {
    stderr
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(3)
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(280)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_deb_package_names_strictly() {
        assert!(validate_deb_package_name("vlc").is_ok());
        assert!(validate_deb_package_name("g++").is_ok());
        assert!(validate_deb_package_name("libfoo2.0:amd64").is_ok());
        assert!(validate_deb_package_name("-y").is_err());
        assert!(validate_deb_package_name("vlc*").is_err());
        assert!(validate_deb_package_name("vlc=3.0").is_err());
        assert!(validate_deb_package_name("vlc;rm").is_err());
        assert!(validate_deb_package_name("a").is_err());
        assert!(validate_deb_package_name("VLC").is_err());
        assert!(validate_deb_package_name("vlc:amd64:extra").is_err());
    }

    #[test]
    fn validates_snap_names_strictly() {
        assert!(validate_snap_name("firefox").is_ok());
        assert!(validate_snap_name("vlc").is_ok());
        assert!(validate_snap_name("editor_beta").is_ok());
        assert!(validate_snap_name("-y").is_err());
        assert!(validate_snap_name("bad--name").is_err());
        assert!(validate_snap_name("bad-").is_err());
        assert!(validate_snap_name("Firefox").is_err());
        assert!(validate_snap_name("a_b_c").is_err());
    }

    #[test]
    fn validates_flatpak_refs_and_installations() {
        assert!(validate_flatpak_ref("app/org.videolan.VLC/x86_64/stable").is_ok());
        assert!(validate_flatpak_ref("--all").is_err());
        assert!(validate_flatpak_ref("../etc/passwd").is_err());
        assert_eq!(flatpak_system_scope_flag(None).unwrap(), "--system");
        assert_eq!(
            flatpak_system_scope_flag(Some("default")).unwrap(),
            "--system"
        );
        assert_eq!(
            flatpak_system_scope_flag(Some("extra")).unwrap(),
            "--installation=extra"
        );
        assert!(flatpak_system_scope_flag(Some("--bad")).is_err());
    }

    #[test]
    fn parses_apt_simulate_removals_installs_and_autoremove() {
        let output = "\
NOTE: This is only a simulation!
Reading package lists... Done
The following packages were automatically installed and are no longer required:
  libvlc5 libvlccore9
  vlc-l10n
Use 'apt autoremove' to remove them.
The following packages will be REMOVED:
  vlc vlc-plugin-qt
Inst dummy-mta (1.0 Ubuntu:24.04 [all])
Remv vlc [3.0.20-3]
Remv vlc-plugin-qt [3.0.20-3]
Purg vlc-data [3.0.20-3]
Remv vlc [3.0.20-3]
";
        let detailed = parse_apt_simulate_removals_detailed(output);
        assert_eq!(
            detailed,
            vec![
                ("vlc".into(), "3.0.20-3".into(), false),
                ("vlc-plugin-qt".into(), "3.0.20-3".into(), false),
                ("vlc-data".into(), "3.0.20-3".into(), true),
            ]
        );
        assert_eq!(
            parse_apt_simulate_removals(output),
            vec!["vlc", "vlc-plugin-qt", "vlc-data"]
        );
        assert_eq!(parse_apt_simulate_installs(output), vec!["dummy-mta"]);
        assert_eq!(
            parse_apt_autoremove_candidates(output),
            vec!["libvlc5", "libvlccore9", "vlc-l10n"]
        );
    }

    #[test]
    fn parses_apt_unmet_dependency_blockers() {
        let output = "\
The following packages have unmet dependencies:
 grub2-common : Depends: python3-apt but it is not going to be installed
 ubuntu-drivers-common : Depends: python3-apt (>= 0.8.5~) but it is not going to be installed
E: Unable to satisfy dependencies.
";
        assert_eq!(
            parse_apt_unmet_dependents(output),
            vec!["grub2-common", "ubuntu-drivers-common"]
        );
    }

    #[test]
    fn formats_bytes_in_french_units() {
        assert_eq!(format_bytes_fr(512), "512 octets");
        assert_eq!(format_bytes_fr(2048), "2,0 Ko");
        assert_eq!(format_bytes_fr(15 * 1024 * 1024), "15 Mo");
        assert_eq!(format_bytes_fr(2 * 1024 * 1024 * 1024), "2,0 Go");
    }

    #[test]
    fn builds_safe_pkexec_arguments_without_autoremove() {
        let deb_args = build_deb_pkexec_args("vlc", false).unwrap();
        assert_eq!(
            deb_args,
            vec![
                "/usr/bin/apt-get",
                "-y",
                "-q",
                "-o",
                "DPkg::Lock::Timeout=0",
                "remove",
                "--",
                "vlc"
            ]
        );
        assert!(!deb_args.iter().any(|a| a.contains("autoremove")));

        let deb_purge = build_deb_pkexec_args("vlc", true).unwrap();
        assert!(deb_purge.contains(&"purge".to_string()));
        assert!(build_deb_pkexec_args("systemd", false).is_err());

        let snap_args = build_snap_pkexec_args("firefox").unwrap();
        assert_eq!(snap_args, vec!["/usr/bin/snap", "remove", "--", "firefox"]);
        assert!(build_snap_pkexec_args("snapd").is_err());

        let (fp_args, scope_flag) = build_flatpak_system_pkexec_args(
            "app/org.videolan.VLC/x86_64/stable",
            Some("extra"),
            true,
        )
        .unwrap();
        assert_eq!(scope_flag, "--installation=extra");
        assert_eq!(
            fp_args,
            vec![
                "/usr/bin/flatpak",
                "uninstall",
                "--installation=extra",
                "-y",
                "--noninteractive",
                "--delete-data",
                "--",
                "app/org.videolan.VLC/x86_64/stable"
            ]
        );
    }

    #[test]
    fn classifies_polkit_cancellation_and_denial() {
        assert!(classify_pkexec_exit(Some(0), "", "APT").is_ok());

        let (status_cancel, err_cancel) = classify_pkexec_exit(Some(126), "", "APT").unwrap_err();
        assert_eq!(status_cancel, OperationStatus::Cancelled);
        assert_eq!(err_cancel.code, "AUTH_CANCELLED");

        let (status_deny, err_deny) = classify_pkexec_exit(Some(127), "", "APT").unwrap_err();
        assert_eq!(status_deny, OperationStatus::Failed);
        assert_eq!(err_deny.code, "AUTH_DENIED");

        let (status_lock, err_lock) = classify_pkexec_exit(
            Some(100),
            "E: Could not get lock /var/lib/dpkg/lock-frontend.",
            "APT",
        )
        .unwrap_err();
        assert_eq!(status_lock, OperationStatus::Failed);
        assert_eq!(err_lock.code, "MANAGER_LOCKED");
    }
}
