//! Utilidades del DOM que Leptos no cubre (desplazamiento a un elemento,
//! portapapeles del sistema y sistema operativo).

use js_sys::{Function, Promise, Reflect};
use wasm_bindgen::{JsCast, JsValue};

/// Sistema operativo donde corre la app (para los textos de la UI).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Windows,
    Mac,
    Linux,
}

pub fn os() -> Os {
    let agent = web_sys::window()
        .and_then(|w| w.navigator().user_agent().ok())
        .unwrap_or_default();
    if agent.contains("Windows") {
        Os::Windows
    } else if agent.contains("Mac") {
        Os::Mac
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
