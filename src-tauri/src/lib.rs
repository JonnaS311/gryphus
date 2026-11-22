//! Composition root de Gryphus: el único lugar que conoce los adaptadores
//! concretos y los inyecta en los casos de uso. Ver docs/architecture/ipc.md.

pub mod commands;
pub mod dto;
pub mod events;
#[cfg(target_os = "macos")]
mod menu;
pub mod shell_env;
pub mod startup;
pub mod state;

use tauri::webview::PageLoadEvent;
use tauri::{Emitter, Manager};

/// Fondo de la ventana desde el primer frame (sin destello blanco mientras carga WebView2).
const BACKGROUND: tauri::window::Color = tauri::window::Color(0x15, 0x17, 0x1c, 0xff);

/// Ventana de Gryphus (la principal y las de proyectos abiertos después).
pub fn window_builder<'a, R: tauri::Runtime, M: Manager<R>>(
    manager: &'a M,
    label: &'a str,
    title: &str,
) -> tauri::WebviewWindowBuilder<'a, R, M> {
    tauri::WebviewWindowBuilder::new(manager, label, tauri::WebviewUrl::App("index.html".into()))
        .title(title)
        .inner_size(1400.0, 900.0)
        .min_inner_size(900.0, 560.0)
        .background_color(BACKGROUND)
}

pub fn run() {
    startup::begin();
    shell_env::import_login_path();
    startup::trace("shell_env");
    let builder = tauri::Builder::default();
    #[cfg(target_os = "macos")]
    let builder = builder.menu(menu::build).on_menu_event(menu::on_event);
    builder
        .plugin(tauri_plugin_dialog::init())
        .on_page_load(|_, payload| match payload.event() {
            PageLoadEvent::Started => startup::trace("page_started"),
            PageLoadEvent::Finished => startup::trace("page_finished"),
        })
        .setup(|app| {
            startup::trace("setup");
            // GRYPHUS_DATA_DIR aísla datos y config (pruebas e2e, perfiles separados).
            let (data_dir, config_file) = match std::env::var_os("GRYPHUS_DATA_DIR") {
                Some(dir) => {
                    let dir = std::path::PathBuf::from(dir);
                    (dir.clone(), dir.join("config.toml"))
                }
                None => (
                    app.path().app_data_dir()?,
                    app.path().app_config_dir()?.join("config.toml"),
                ),
            };
            app.manage(state::AppState::new(&data_dir, config_file));
            // La ventana se crea aquí (no en tauri.conf.json) para medir su coste:
            // crearla lanza WebView2 (~250 ms, ~85 % del arranque en frío).
            startup::trace("window_build_start");
            window_builder(app, "main", "Gryphus").build()?;
            startup::trace("window_built");
            Ok(())
        })
        .on_window_event(|window, event| match event {
            // Con agentes activos o cambios sin guardar, la UI confirma antes de cerrar
            // (antes la X los detenía/descartaba sin preguntar).
            tauri::WindowEvent::CloseRequested { api, .. }
                if window
                    .state::<state::AppState>()
                    .close_guarded(window.label()) =>
            {
                api.prevent_close();
                let _ = window.emit_to(window.label(), gryphus_ipc::event::CLOSE_REQUESTED, ());
            }
            tauri::WindowEvent::Destroyed => {
                window
                    .state::<state::AppState>()
                    .close_window(window.label());
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            commands::pick_folder,
            commands::open_project,
            commands::get_project,
            commands::close_project,
            commands::restore_last_project,
            commands::create_feature,
            commands::delete_feature,
            commands::spawn_agent,
            commands::stop_agent,
            commands::restart_agent,
            commands::remove_agent,
            commands::rename_agent,
            commands::agent_input,
            commands::agent_resize,
            commands::attach_agent,
            commands::detach_agent,
            commands::list_dir,
            commands::read_file,
            commands::write_file,
            commands::rename_path,
            commands::copy_path,
            commands::import_paths,
            commands::clipboard_files,
            commands::clipboard_copy_path,
            commands::git_status,
            commands::git_diff,
            commands::git_overview,
            commands::ui_ready,
            commands::set_close_guard,
            commands::confirm_close,
        ])
        .run(tauri::generate_context!())
        .expect("error al iniciar Gryphus");
}
