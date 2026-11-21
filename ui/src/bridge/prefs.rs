//! Preferencias de la ventana: `localStorage` y variables CSS de `:root`.

/// Evento que escucha la terminal (ui/js) para releer `--mono` y `--mono-size`.
const FONTS_EVENT: &str = "gryphus:fonts";

/// Tamaño interior de la ventana en px (ancho, alto), para colocar menús flotantes.
pub fn viewport() -> (i32, i32) {
    let dim = |v: Result<wasm_bindgen::JsValue, wasm_bindgen::JsValue>| {
        v.ok()
            .and_then(|v| v.as_f64())
            .map_or(i32::MAX, |v| v as i32)
    };
    web_sys::window().map_or((i32::MAX, i32::MAX), |w| {
        (dim(w.inner_width()), dim(w.inner_height()))
    })
}

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

pub fn load(key: &str) -> Option<String> {
    storage()?.get_item(key).ok().flatten()
}

pub fn save(key: &str, value: &str) {
    if let Some(s) = storage() {
        let _ = s.set_item(key, value);
    }
}

/// Aplica variables CSS en `:root` y avisa a la terminal.
pub fn apply_css_vars<'a>(vars: impl IntoIterator<Item = (&'a str, String)>) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(root) = window
        .document()
        .and_then(|d| d.document_element())
        .and_then(|e| wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlElement>(e).ok())
    else {
        return;
    };
    let style = root.style();
    for (name, value) in vars {
        let _ = style.set_property(name, &value);
    }
    if let Ok(ev) = web_sys::Event::new(FONTS_EVENT) {
        let _ = window.dispatch_event(&ev);
    }
}
