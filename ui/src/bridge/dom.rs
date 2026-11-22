//! Utilidades del DOM que Leptos no cubre (desplazamiento a un elemento, foco,
//! plataforma, modificador principal de los atajos y qué hay bajo el puntero al soltar archivos).

use std::sync::OnceLock;

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

/// Qué hay bajo un punto de la ventana al arrastrar archivos encima.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hit {
    /// La terminal del agente.
    Terminal,
    /// El explorador; con la fila bajo el puntero (`data-path`, ¿es carpeta?) si la hay.
    Explorer(Option<(String, bool)>),
}

/// Hit-test en píxeles físicos relativos a la ventana (como los envía Tauri en
/// `tauri://drag-*`).
pub fn hit_test(physical_x: f64, physical_y: f64) -> Option<Hit> {
    let window = web_sys::window()?;
    let ratio = window.device_pixel_ratio().max(0.1);
    let el = window
        .document()?
        .element_from_point((physical_x / ratio) as f32, (physical_y / ratio) as f32)?;
    if el.closest(".terminal").ok().flatten().is_some() {
        return Some(Hit::Terminal);
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
