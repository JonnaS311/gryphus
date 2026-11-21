//! Gancho e2e de las confirmaciones.
//!
//! Las confirmaciones son un modal propio de la UI (`app::ConfirmDialog`), no
//! `window.confirm`: `tauri-plugin-dialog` lo sustituye por un diálogo nativo
//! asíncrono que no sigue el tema y que, tratado como síncrono, daba siempre "cancelar".

use js_sys::{Array, Reflect};
use wasm_bindgen::{JsCast, JsValue};

/// Si las pruebas e2e activaron `window.__gryphusAutoConfirm`, registra el mensaje
/// en `window.__confirms` y acepta sin mostrar el modal.
pub fn e2e_auto_confirm(message: &str) -> Option<bool> {
    let window = web_sys::window()?;
    let enabled = Reflect::get(&window, &JsValue::from_str("__gryphusAutoConfirm"))
        .ok()?
        .as_bool()?;
    if !enabled {
        return None;
    }
    if let Ok(log) = Reflect::get(&window, &JsValue::from_str("__confirms")) {
        if let Ok(log) = log.dyn_into::<Array>() {
            log.push(&JsValue::from_str(message));
        }
    }
    Some(true)
}
