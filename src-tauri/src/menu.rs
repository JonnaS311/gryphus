//! Menú de la app en macOS.
//!
//! En macOS el WebView solo recibe Cmd+C/V/X/Z/A si existen en la barra de menús
//! (menú Edición). El menú por defecto de Tauri también trae "Cerrar ventana"
//! con Cmd+W, que se comería el atajo de cerrar pestaña del editor, y un "Salir"
//! que terminaría la app sin pasar por la guarda de cierre (agentes activos o
//! cambios sin guardar). Por eso se define uno propio.

use tauri::menu::{AboutMetadata, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Manager, Runtime};

const QUIT: &str = "gryphus-quit";

pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let info = app.package_info();
    let about = AboutMetadata {
        name: Some(info.name.clone()),
        version: Some(info.version.to_string()),
        copyright: Some("Copyright (c) 2026 Jonnathan Sotelo. Licencia MIT.".into()),
        ..Default::default()
    };
    let sep = || PredefinedMenuItem::separator(app);
    let quit = MenuItem::with_id(app, QUIT, "Salir de Gryphus", true, Some("CmdOrCtrl+Q"))?;
    let app_menu = Submenu::with_items(
        app,
        "Gryphus",
        true,
        &[
            &PredefinedMenuItem::about(app, Some("Acerca de Gryphus"), Some(about))?,
            &sep()?,
            &PredefinedMenuItem::services(app, Some("Servicios"))?,
            &sep()?,
            &PredefinedMenuItem::hide(app, Some("Ocultar Gryphus"))?,
            &PredefinedMenuItem::hide_others(app, Some("Ocultar otros"))?,
            &PredefinedMenuItem::show_all(app, Some("Mostrar todo"))?,
            &sep()?,
            &quit,
        ],
    )?;
    let edit = Submenu::with_items(
        app,
        "Edición",
        true,
        &[
            &PredefinedMenuItem::undo(app, Some("Deshacer"))?,
            &PredefinedMenuItem::redo(app, Some("Rehacer"))?,
            &sep()?,
            &PredefinedMenuItem::cut(app, Some("Cortar"))?,
            &PredefinedMenuItem::copy(app, Some("Copiar"))?,
            &PredefinedMenuItem::paste(app, Some("Pegar"))?,
            &PredefinedMenuItem::select_all(app, Some("Seleccionar todo"))?,
        ],
    )?;
    let window = Submenu::with_items(
        app,
        "Ventana",
        true,
        &[
            &PredefinedMenuItem::minimize(app, Some("Minimizar"))?,
            &PredefinedMenuItem::maximize(app, Some("Zoom"))?,
            &sep()?,
            &PredefinedMenuItem::fullscreen(app, Some("Pantalla completa"))?,
        ],
    )?;
    Menu::with_items(app, &[&app_menu, &edit, &window])
}

/// "Salir" cierra cada ventana como su botón rojo: cada una pasa por su guarda.
pub fn on_event<R: Runtime>(app: &AppHandle<R>, event: MenuEvent) {
    if event.id() == QUIT {
        for window in app.webview_windows().values() {
            let _ = window.close();
        }
    }
}
