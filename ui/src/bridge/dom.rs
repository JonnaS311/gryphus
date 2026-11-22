//! Utilidades del DOM que Leptos no cubre (desplazamiento a un elemento, foco,
//! portapapeles del sistema, plataforma y modificador principal de los atajos).

use std::sync::OnceLock;

use js_sys::{Function, Promise, Reflect};
use wasm_bindgen::{JsCast, JsValue};

/// `true` en macOS (WKWebView). Allí los atajos usan ⌘ en vez de Ctrl.
pub fn is_mac() -> bool {
    static MAC: OnceLock<bool> = OnceLock::new();
    *MAC.get_or_init(|| {
        web_sys::window()
            .and_then(|w| w.navigator().user_agent().ok())
            .is_some_and(|ua| ua.contains("Macintosh") || ua.contains("Mac OS X"))
    })
}

/// Modificador principal pulsado: ⌘ en macOS, Ctrl en el resto.
pub fn primary_mod(ev: &web_sys::KeyboardEvent) -> bool {
    if is_mac() {
        ev.meta_key()
    } else {
        ev.ctrl_key()
    }
}

/// `true` si la ventana tiene el foco (el usuario la está mirando).
pub fn has_focus() -> bool {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.has_focus().ok())
        .unwrap_or(true)
}

/// Sistema operativo donde corre la app (para los textos de la UI).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Windows,
    Mac,
    Linux,
}

pub fn os() -> Os {
    if is_mac() {
        return Os::Mac;
    }
    let windows = web_sys::window()
        .and_then(|w| w.navigator().user_agent().ok())
        .is_some_and(|ua| ua.contains("Windows"));
    if windows {
        Os::Windows
    } else {
        Os::Linux
    }
}

/// Copia texto al portapapeles del sistema (`navigator.clipboard.writeText`).
pub async fn copy_text(text: &str) -> Result<(), String> {
    let call = || -> Result<Promise, JsValue> {
        let navigator = Reflect::get(&js_sys::global(), &"navigator".into())?;
        let clipboard = Reflect::get(&navigator, &"clipboard".into())?;
        let write: Function = Reflect::get(&clipboard, &"writeText".into())?.dyn_into()?;
        write.call1(&clipboard, &text.into())?.dyn_into()
    };
    let promise = call().map_err(|_| "el portapapeles no está disponible".to_owned())?;
    wasm_bindgen_futures::JsFuture::from(promise)
        .await
        .map(drop)
        .map_err(|e| {
            e.as_string()
                .or_else(|| {
                    Reflect::get(&e, &"message".into())
                        .ok()
                        .and_then(|m| m.as_string())
                })
                .unwrap_or_else(|| "no se pudo copiar al portapapeles".to_owned())
        })
}

/// Abre (si es un `<details>`) y desplaza hasta el elemento con ese `id`.
/// Se busca por `id` para no tener que escapar rutas en un selector CSS.
pub fn reveal(id: &str) {
    let Some(el) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id(id))
    else {
        return;
    };
    if el.tag_name().eq_ignore_ascii_case("details") {
        let _ = el.set_attribute("open", "");
    }
    el.scroll_into_view_with_bool(true);
}
