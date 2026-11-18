use super::{command::read_command, ProviderInventory};
use crate::domain::{AppError, ApplicationInstallation, PackageFormat};
use std::collections::HashSet;

pub fn inventory() -> Result<ProviderInventory, AppError> {
    let output = read_command("/usr/bin/flatpak", &[
        "list", "--app", "--columns=ref:f,application:f,name:f,version:f,installation:f,origin:f,size,description:f",
    ])?;
    parse(&output)
}

fn parse(output: &str) -> Result<ProviderInventory, AppError> {
    let mut result = ProviderInventory::default();
    let mut identities = HashSet::new();
    for line in output.lines().filter(|line| !line.trim().is_empty()) {
        let fields: Vec<_> = line.splitn(8, '\t').collect();
        if fields.len() != 8 {
            return Err(AppError::new(
                "FLATPAK_FORMAT",
                "Le format de réponse Flatpak est inattendu.",
            ));
        }
        let reference = fields[0].strip_prefix("app/").unwrap_or(fields[0]);
        let parts: Vec<_> = reference.split('/').collect();
        if parts.len() != 3
            || parts.iter().any(|part| part.is_empty())
            || parts[0] != fields[1]
            || fields[4].is_empty()
        {
            return Err(AppError::new(
                "FLATPAK_REFERENCE",
                "Une référence Flatpak n’a pas pu être identifiée précisément.",
            ));
        }
        let full_reference = format!("app/{reference}");
        let scope = if fields[4] == "user" {
            "user"
        } else {
            "system"
        };
        let installation = if fields[4] == "system" {
            "default"
        } else {
            fields[4]
        };
        let id = format!("flatpak:{scope}:{installation}:{full_reference}");
        if !identities.insert(id.clone()) {
            continue;
        }
        let mut item = ApplicationInstallation::read_only(
            PackageFormat::Flatpak,
            id,
            fields[1].into(),
            if fields[2].is_empty() {
                fields[1]
            } else {
                fields[2]
            }
            .into(),
        );
        item.scope = scope.into();
        item.installation = Some(installation.into());
        item.reference = Some(full_reference);
        item.version = fields[3].into();
        item.description = fields[7].into();
        item.reported_size = (!fields[6].is_empty()).then(|| fields[6].into());
        let is_user = scope == "user";
        item.can_uninstall = is_user;
        item.unavailable_reason = if is_user {
            String::new()
        } else {
            "La désinstallation d’une application Flatpak système requiert des privilèges administrateur.".into()
        };
        result.installations.push(item);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_scope_branch_architecture_and_named_installation() {
        let text = "org.test.App/x86_64/stable\torg.test.App\tApp\t1\tuser\tflathub\t12 MB\tDescription\norg.test.App/x86_64/stable\torg.test.App\tApp\t1\tsystem\tflathub\t12 MB\tDescription\norg.test.App/aarch64/beta\torg.test.App\tApp\t2\textra\tremote\t\t\n";
        let result = parse(text).unwrap();
        let items = result.installations;
        assert_eq!(items.len(), 3);
        assert_ne!(items[0].id, items[1].id);
        assert_eq!(items[1].installation.as_deref(), Some("default"));
        assert_eq!(items[2].installation.as_deref(), Some("extra"));
        assert_eq!(
            items[2].reference.as_deref(),
            Some("app/org.test.App/aarch64/beta")
        );
        assert!(items[0].installed_size.is_none()); // La CLI fournit une taille arrondie.
        assert_eq!(items[0].reported_size.as_deref(), Some("12 MB"));
    }

    #[test]
    fn accepts_empty_inventory_but_rejects_ambiguous_rows() {
        assert!(parse("").unwrap().installations.is_empty());
        assert!(parse("org.test.App\tbroken").is_err());
        assert!(parse("invalid\torg.test.App\tApp\t1\tuser\tremote\t1 MB\tDesc").is_err());
    }
}
