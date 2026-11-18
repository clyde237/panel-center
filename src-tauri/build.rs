fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "list_installations",
            "get_system_info",
            "prepare_uninstall",
            "confirm_uninstall",
            "get_operation",
            "list_history",
            "clear_history",
            "get_preferences",
            "update_preferences",
        ]),
    ))
    .expect("Impossible de construire les permissions de Panel Center");
}
