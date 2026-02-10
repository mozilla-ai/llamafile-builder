#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod commands;
mod download;
mod models;
mod builder;

use tracing_subscriber;

fn main() {
    // Initialize logging
    tracing_subscriber::fmt::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .manage(commands::AppState::default())
        .setup(|app| {
            use tauri::Manager;
            let app_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&app_dir).ok();
            std::fs::create_dir_all(app_dir.join("cache")).ok();
            std::fs::create_dir_all(app_dir.join("downloads")).ok();
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_llamafile_versions,
            commands::get_models,
            commands::get_config_schema,
            commands::search_models,
            commands::get_model_details,
            commands::start_build,
            commands::cancel_build,
            commands::get_build_status,
            commands::select_output_path,
            commands::get_default_output_path,
            commands::get_system_info,
            commands::validate_config,
            commands::select_llamafile_path,
            commands::get_llamafile_cache_dir,
            commands::get_model_cache_dir,
            commands::select_model_file_path,
            commands::select_model_download_dir,
            commands::run_zipalign,
            commands::download_file,
            commands::file_exists,
            commands::get_hf_token,
            commands::set_hf_token,
            commands::clear_hf_token,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
