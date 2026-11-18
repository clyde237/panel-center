use crate::{
    domain::{AppError, Inventory, PackageFormat, ProviderState, ProviderStatus},
    providers::{appimage, deb, flatpak, local, snap, ProviderInventory},
};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn collect() -> Inventory {
    collect_with_paths(&[])
}

pub fn collect_with_paths(paths: &[String]) -> Inventory {
    let (deb_result, snap_result, flatpak_result, appimage_result, local_result) =
        std::thread::scope(|scope| {
            let deb_job = scope.spawn(|| {
                deb::inventory().map(|value| ProviderInventory {
                    installations: value.installations,
                    warnings: value.warning.into_iter().collect(),
                    locations: vec![],
                })
            });
            let snap_job = scope.spawn(snap::inventory);
            let flatpak_job = scope.spawn(flatpak::inventory);
            let appimage_job = scope.spawn(|| appimage::inventory(paths));
            let local_job = scope.spawn(local::inventory);
            let recover =
                |job: std::thread::ScopedJoinHandle<'_, Result<ProviderInventory, AppError>>| {
                    job.join().unwrap_or_else(|_| {
                        Err(AppError::new(
                            "PROVIDER_FAILED",
                            "La lecture du connecteur a été interrompue.",
                        ))
                    })
                };
            (
                recover(deb_job),
                recover(snap_job),
                recover(flatpak_job),
                recover(appimage_job),
                recover(local_job),
            )
        });
    let mut inventory = Inventory {
        installations: vec![],
        providers: vec![],
        appimage_locations: vec![],
        generated_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    };
    for (format, result) in [
        (PackageFormat::Deb, deb_result),
        (PackageFormat::Snap, snap_result),
        (PackageFormat::Flatpak, flatpak_result),
        (PackageFormat::Appimage, appimage_result),
        (PackageFormat::Local, local_result),
    ] {
        merge(&mut inventory, format, result);
    }
    inventory
        .installations
        .sort_by_key(|item| (item.name.to_lowercase(), item.id.clone()));
    inventory
}

fn merge(
    inventory: &mut Inventory,
    format: PackageFormat,
    result: Result<ProviderInventory, AppError>,
) {
    let status = match result {
        Ok(mut data) => {
            inventory.installations.append(&mut data.installations);
            inventory.appimage_locations.append(&mut data.locations);
            ProviderStatus {
                format,
                state: if data.warnings.is_empty() {
                    ProviderState::Ready
                } else {
                    ProviderState::Partial
                },
                message: if data.warnings.is_empty() {
                    "Connecté et disponible.".into()
                } else {
                    data.warnings.join(" ")
                },
            }
        }
        Err(error) => ProviderStatus {
            format,
            state: if matches!(
                error.code.as_str(),
                "MANAGER_UNAVAILABLE" | "MANAGER_ACCESS" | "SNAP_UNAVAILABLE"
            ) {
                ProviderState::Unavailable
            } else {
                ProviderState::Error
            },
            message: format!("{format:?} : {}", error.message),
        },
    };
    inventory.providers.push(status);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retains_successful_provider_results_when_another_fails() {
        let mut result = Inventory {
            installations: vec![],
            providers: vec![],
            generated_at: 0,
            appimage_locations: vec![],
        };
        let installed = crate::domain::ApplicationInstallation::read_only(
            PackageFormat::Flatpak,
            "flatpak:user:test".into(),
            "test".into(),
            "Test".into(),
        );
        merge(
            &mut result,
            PackageFormat::Flatpak,
            Ok(ProviderInventory {
                installations: vec![installed],
                ..ProviderInventory::default()
            }),
        );
        merge(
            &mut result,
            PackageFormat::Snap,
            Err(AppError::new("SNAP_UNAVAILABLE", "absent")),
        );
        assert_eq!(result.providers.len(), 2);
        assert_eq!(result.installations.len(), 1);
        assert_eq!(result.installations[0].id, "flatpak:user:test");
        assert!(matches!(result.providers[0].state, ProviderState::Ready));
        assert!(matches!(
            result.providers[1].state,
            ProviderState::Unavailable
        ));
    }
}
