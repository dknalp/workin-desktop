mod auth;
mod download;
mod error;
mod state;
mod tray;
mod upload;

use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .manage(AppState::default())
        .setup(|app| {
            tray::setup_tray(&app.handle())?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // Hide to tray instead of quitting
                window.hide().unwrap();
                api.prevent_close();
            }
        })
        .invoke_handler(tauri::generate_handler![
            auth::cmd_login,
            auth::cmd_logout,
            auth::cmd_restore_session,
            upload::cmd_upload_file,
            upload::cmd_list_files,
            download::cmd_download_file,
            download::cmd_get_downloads_dir,
            download::cmd_create_folder,
            download::cmd_rename_file,
            download::cmd_delete_file,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
