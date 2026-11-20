//! Composition root de Gryphus: el único lugar que conoce los adaptadores
//! concretos y los inyecta en los casos de uso. Ver docs/architecture/ipc.md.

pub mod commands;
pub mod dto;
pub mod events;
pub mod state;

use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let config_file = app.path().app_config_dir()?.join("config.toml");
            app.manage(state::AppState::new(&data_dir, config_file));
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                window
                    .state::<state::AppState>()
                    .close_window(window.label());
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::pick_folder,
            commands::open_project,
            commands::get_project,
            commands::create_feature,
            commands::delete_feature,
            commands::spawn_agent,
            commands::stop_agent,
            commands::restart_agent,
            commands::remove_agent,
            commands::agent_input,
            commands::agent_resize,
            commands::attach_agent,
            commands::detach_agent,
            commands::list_dir,
            commands::read_file,
            commands::write_file,
            commands::git_status,
            commands::git_diff,
            commands::git_overview,
        ])
        .run(tauri::generate_context!())
        .expect("error al iniciar Gryphus");
}
