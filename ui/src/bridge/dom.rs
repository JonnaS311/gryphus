//! Utilidades del DOM que Leptos no cubre (desplazamiento a un elemento,
//! plataforma y modificador principal de los atajos).

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
