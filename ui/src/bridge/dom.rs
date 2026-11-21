//! Utilidades del DOM que Leptos no cubre (desplazamiento a un elemento).

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
