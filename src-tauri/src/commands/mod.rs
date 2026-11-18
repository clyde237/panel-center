use crate::domain::{
    AppError, ApplicationInstallation, HistoryEntry, Inventory, Operation, Preferences, SystemInfo,
    UninstallPlan, UninstallPlanOptions,
};
use crate::services::transactions::manager;
use crate::storage;

#[tauri::command]
pub async fn list_installations(
    appimage_paths: Option<Vec<String>>,
) -> Result<Inventory, AppError> {
    let mut paths = appimage_paths.unwrap_or_default();
    // Merge configured AppImage folders from persisted preferences
    let prefs = storage::load_preferences();
    for folder in prefs.appimage_folders {
        if !paths.contains(&folder) {
            paths.push(folder);
        }
    }

    if paths.len() > 32
        || paths
            .iter()
            .any(|path| path.len() > 4096 || path.contains('\0'))
    {
        return Err(AppError::new(
            "INVALID_PATHS",
            "Ajoute des emplacements AppImage valides.",
        ));
    }
    tauri::async_runtime::spawn_blocking(move || {
        crate::services::inventory::collect_with_paths(&paths)
    })
    .await
    .map_err(|_| {
        AppError::new(
            "INVENTORY_FAILED",
            "La lecture de l’inventaire a été interrompue.",
        )
    })
}

#[tauri::command]
pub fn get_system_info() -> SystemInfo {
    let release = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
    let os = release
        .lines()
        .find_map(|line| line.strip_prefix("PRETTY_NAME="))
        .map(|name| name.trim_matches('"').to_string())
        .unwrap_or_else(|| std::env::consts::OS.to_string());
    SystemInfo {
        os,
        architecture: std::env::consts::ARCH.into(),
        app_version: env!("CARGO_PKG_VERSION").into(),
        mode: "development".into(),
    }
}

#[tauri::command]
pub fn prepare_uninstall(
    installation: ApplicationInstallation,
    options: Option<UninstallPlanOptions>,
) -> Result<UninstallPlan, AppError> {
    let opts = options.unwrap_or_default();
    manager().prepare(&installation, opts)
}

#[tauri::command]
pub fn confirm_uninstall(plan_id: String) -> Result<Operation, AppError> {
    manager().confirm(&plan_id)
}

#[tauri::command]
pub fn get_operation() -> Option<Operation> {
    manager().get_current_operation()
}

#[tauri::command]
pub fn list_history() -> Vec<HistoryEntry> {
    storage::load_history()
}

#[tauri::command]
pub fn clear_history() -> Result<(), AppError> {
    storage::clear_history()
}

#[tauri::command]
pub fn get_preferences() -> Preferences {
    storage::load_preferences()
}

#[tauri::command]
pub fn update_preferences(preferences: Preferences) -> Result<(), AppError> {
    storage::save_preferences(&preferences)
}
