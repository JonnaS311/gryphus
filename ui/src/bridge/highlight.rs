//! Colores de sintaxis del diff con los parsers de CodeMirror
//! (ver ui/js/src/index.js → highlightHunk). Requiere el pegamento ya cargado
//! (`Store::glue_ready`).

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(catch, js_namespace = GryphusJs, js_name = highlightHunk)]
    fn highlight_hunk_js(name: &str, kinds: &str, texts: Vec<String>) -> Result<JsValue, JsValue>;
}

/// Tramo de una línea: texto y clases CSS (`hl-*`; vacío = sin color).
pub type Span = (String, String);

/// Una entrada por línea (`kinds`: `+`, `-` o ` ` por línea). `None` si el
/// pegamento falla o devuelve algo inesperado: el diff se muestra sin colores.
pub fn highlight_hunk(name: &str, kinds: &str, texts: Vec<String>) -> Option<Vec<Vec<Span>>> {
    let value = highlight_hunk_js(name, kinds, texts).ok()?;
    serde_wasm_bindgen::from_value(value).ok()
}
