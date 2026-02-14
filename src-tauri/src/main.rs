#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod build_state;
mod builder;
mod commands;
mod download;
mod models;
mod validation;

use tracing_subscriber;

fn main() {
    // Initialize logging
    tracing_subscriber::fmt::init();

    // Validate all JSON config files before starting
    validation::validate_config_files();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
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
            commands::generate_args_content,
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
            commands::cancel_download,
            commands::get_file_info,
            commands::file_exists,
            commands::delete_file,
            commands::copy_file,
            commands::write_temp_file,
            commands::make_executable,
            commands::calculate_sha256,
            commands::get_hf_token,
            commands::set_hf_token,
            commands::clear_hf_token,
            commands::open_url,
            // Build state commands
            commands::get_build_state,
            commands::select_registry_model,
            commands::select_custom_model,
            commands::add_custom_companion_file,
            commands::remove_custom_companion_file,
            commands::select_file_quantization,
            commands::use_custom_file_for_role,
            commands::clear_custom_file_for_role,
            commands::select_llamafile_version,
            commands::use_custom_llamafile,
            commands::set_config_value,
            commands::set_output_path,
            // Ollama commands
            commands::get_default_ollama_cache_dir,
            commands::check_ollama_available,
            commands::get_ollama_models,
            commands::select_ollama_model,
            // LM Studio commands
            commands::get_default_lmstudio_cache_dir,
            commands::check_lmstudio_available,
            commands::get_lmstudio_models,
            commands::select_lmstudio_model,
            commands::select_lmstudio_file,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
