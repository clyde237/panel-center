use std::{collections::HashMap, fs, path::Path, process::Command};

use crate::domain::{AppError, ApplicationInstallation, PackageFormat};

const DPKG_QUERY: &str = "/usr/bin/dpkg-query";
const QUERY_FORMAT: &str =
    "${binary:Package}\t${Version}\t${Installed-Size}\t${db:Status-Abbrev}\t${binary:Synopsis}\n";

pub struct DebInventory {
    pub installations: Vec<ApplicationInstallation>,
    pub warning: Option<String>,
}

pub fn inventory() -> Result<DebInventory, AppError> {
    let output = Command::new(DPKG_QUERY)
        .args(["--show", "--showformat", QUERY_FORMAT])
        .env("LC_ALL", "C")
        .output()
        .map_err(|_| AppError::new("DEB_UNAVAILABLE", "Le gestionnaire DEB est indisponible."))?;
    if !output.status.success() {
        return Err(AppError::new(
            "DEB_QUERY_FAILED",
            "Impossible d’interroger les paquets DEB.",
        ));
    }
    let content = String::from_utf8(output.stdout).map_err(|_| {
        AppError::new(
            "DEB_ENCODING",
            "L’inventaire DEB contient un encodage inattendu.",
        )
    })?;

    let owners = Command::new(DPKG_QUERY)
        .args(["--search", "/usr/share/applications/*.desktop"])
        .env("LC_ALL", "C")
        .output();
    let (names, warning) = match owners {
        Ok(result) if result.status.success() => (
            desktop_names(&String::from_utf8_lossy(&result.stdout)),
            None,
        ),
        _ => (HashMap::new(), Some("Les lanceurs n’ont pas pu être associés aux paquets. Active « Inclure les paquets techniques » pour consulter l’inventaire.".into())),
    };

    Ok(DebInventory {
        installations: parse_packages(&content, &names),
        warning,
    })
}

fn desktop_names(content: &str) -> HashMap<String, String> {
    let mut paths: Vec<_> = content
        .lines()
        .filter_map(|line| line.split_once(": "))
        .collect();
    paths.sort_unstable();
    let mut names = HashMap::new();
    for (owners, path) in paths {
        let path = Path::new(path);
        if !path.starts_with("/usr/share/applications")
            || path.extension().is_none_or(|ext| ext != "desktop")
        {
            continue;
        }
        let Ok(entry) = fs::read_to_string(path) else {
            continue;
        };
        let Some(name) = parse_desktop_name(&entry) else {
            continue;
        };
        for owner in owners.split(", ") {
            names
                .entry(owner.to_string())
                .or_insert_with(|| name.clone());
        }
    }
    names
}

fn parse_desktop_name(content: &str) -> Option<String> {
    let mut in_entry = false;
    let mut fields = HashMap::new();
    for line in content.lines().map(str::trim) {
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry {
            if let Some((key, value)) = line.split_once('=') {
                fields.insert(key, value);
            }
        }
    }
    if fields.get("Type") != Some(&"Application")
        || fields.get("Hidden") == Some(&"true")
        || fields.get("NoDisplay") == Some(&"true")
    {
        return None;
    }
    fields
        .get("Name[fr]")
        .or_else(|| fields.get("Name"))
        .filter(|name| !name.is_empty())
        .map(|name| name.to_string())
}

fn parse_packages(content: &str, names: &HashMap<String, String>) -> Vec<ApplicationInstallation> {
    let mut packages = Vec::new();
    for line in content.lines() {
        let columns: Vec<_> = line.splitn(5, '\t').collect();
        // Le deuxième caractère est l’état réel ; le premier peut être « h » (hold).
        if columns.len() != 5
            || columns[3].as_bytes().get(1) != Some(&b'i')
            || columns[0].is_empty()
        {
            continue;
        }
        let native_id = columns[0];
        let name = names
            .get(native_id)
            .or_else(|| names.get(native_id.split(':').next().unwrap_or(native_id)));
        let is_protected = crate::domain::protection::is_protected_deb(native_id);
        packages.push(ApplicationInstallation {
            id: format!("deb:system:{native_id}"),
            name: name.cloned().unwrap_or_else(|| native_id.to_string()),
            native_id: native_id.into(),
            version: columns[1].into(),
            format: PackageFormat::Deb,
            scope: "system".into(),
            description: columns[4].into(),
            installed_size: columns[2].parse::<u64>().ok().and_then(|kb| kb.checked_mul(1024)),
            reported_size: None,
            installation: Some("system".into()),
            reference: None,
            path: None,
            is_executable: None,
            is_desktop_application: name.is_some(),
            can_uninstall: !is_protected,
            unavailable_reason: if is_protected {
                "Ce paquet est essentiel au fonctionnement du système Ubuntu et ne peut pas être supprimé.".into()
            } else {
                String::new()
            },
        });
    }
    packages.sort_by_key(|package| (package.name.to_lowercase(), package.id.clone()));
    packages
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_only_installed_packages_and_preserves_architectures() {
        let names = HashMap::from([("editor".into(), "Éditeur".into())]);
        let data = "editor\t1.2\t42\tii \tText editor\nold\t1\t10\trc \tRemoved\nliba:amd64\t2\t5\tii \tLibrary\nliba:i386\t2\t5\tii \tLibrary\ninvalid";
        let result = parse_packages(data, &names);
        assert_eq!(result.len(), 3);
        let editor = result.iter().find(|p| p.native_id == "editor").unwrap();
        assert_eq!(editor.name, "Éditeur");
        assert!(editor.is_desktop_application);
        assert_eq!(editor.installed_size, Some(42 * 1024));
        assert!(editor.can_uninstall);
        assert_ne!(result[0].id, result[1].id);
        let protected_data = "apt\t2.8\t1024\tii \tPackage manager";
        let protected = parse_packages(protected_data, &HashMap::new());
        assert!(!protected[0].can_uninstall);
    }

    #[test]
    fn ignores_hidden_launchers_and_desktop_actions() {
        assert_eq!(parse_desktop_name("[Desktop Entry]\nType=Application\nName=Editor\nName[fr]=Éditeur\n[Desktop Action New]\nName=New"), Some("Éditeur".into()));
        assert_eq!(
            parse_desktop_name("[Desktop Entry]\nType=Application\nName=Hidden\nNoDisplay=true"),
            None
        );
        assert_eq!(
            parse_desktop_name("[Desktop Entry]\nType=Application\nName=Hidden\nHidden=true"),
            None
        );
    }

    #[test]
    fn handles_unknown_or_overflowing_sizes() {
        let data = "a\t1\tn/a\tii \tA\nb\t1\t18446744073709551615\tii \tB";
        assert!(parse_packages(data, &HashMap::new())
            .iter()
            .all(|p| p.installed_size.is_none()));
    }

    #[test]
    fn includes_installed_packages_marked_as_held() {
        let data = "held-editor\t1\t42\thi \tHeld package";
        assert_eq!(parse_packages(data, &HashMap::new()).len(), 1);
    }
}
