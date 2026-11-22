//! Utilidades del DOM que Leptos no cubre (desplazamiento a un elemento, foco,
//! portapapeles del sistema, plataforma, modificador principal de los atajos y qué
//! hay bajo el puntero al soltar archivos).

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

/// Qué hay bajo un punto de la ventana al arrastrar archivos encima.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hit {
    /// La terminal del agente.
    Terminal,
    /// El editor (pestañas, cabecera o documento).
    Editor,
    /// El explorador; con la fila bajo el puntero (`data-path`, ¿es carpeta?) si la hay.
    Explorer(Option<(String, bool)>),
}

/// Hit-test en píxeles físicos relativos a la ventana (como los envía Tauri en
/// `tauri://drag-*`).
pub fn hit_test(physical_x: f64, physical_y: f64) -> Option<Hit> {
    let ratio = web_sys::window()?.device_pixel_ratio().max(0.1);
    hit_test_css(physical_x / ratio, physical_y / ratio)
}

/// Hit-test en píxeles CSS (`clientX`/`clientY` de los eventos del puntero).
pub fn hit_test_css(x: f64, y: f64) -> Option<Hit> {
    let el = web_sys::window()?
        .document()?
        .element_from_point(x as f32, y as f32)?;
    if el.closest(".terminal").ok().flatten().is_some() {
        return Some(Hit::Terminal);
    }
    if el.closest(".view-editor").ok().flatten().is_some() {
        return Some(Hit::Editor);
    }
    el.closest(".pane-right").ok().flatten()?;
    let row = el.closest(".tree-row").ok().flatten().and_then(|row| {
        let path = row.get_attribute("data-path")?;
        let is_dir = row
            .get_attribute("class")
            .is_some_and(|c| c.split_whitespace().any(|c| c == "dir"));
        Some((path, is_dir))
    });
    Some(Hit::Explorer(row))
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
