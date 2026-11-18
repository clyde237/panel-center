pub mod commands;
pub mod domain;
pub mod providers;
pub mod services;
pub mod storage;

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::list_installations,
            commands::get_system_info,
            commands::prepare_uninstall,
            commands::confirm_uninstall,
            commands::get_operation,
            commands::list_history,
            commands::clear_history,
            commands::get_preferences,
            commands::update_preferences,
        ])
        .run(tauri::generate_context!())
        .expect("Impossible de lancer Panel Center");
}
