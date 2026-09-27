//! DroidBridge — library root
//!
//! Sets up the Tauri application, registers plugins and commands,
//! and initialises shared application state.

mod commands;
pub mod mtp;
mod state;

use state::AppState;

pub fn run() {
    env_logger::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            commands::scan_devices,
            commands::connect_device,
            commands::disconnect_device,
            commands::list_storages,
            commands::list_files,
            commands::download_files,
            commands::upload_files,
            commands::cancel_transfer,
            commands::delete_objects,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run DroidBridge");
}
