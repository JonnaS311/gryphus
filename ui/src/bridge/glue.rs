//! Carga bajo demanda del pegamento JS (xterm.js + CodeMirror, ~1,1 MB).
//!
//! No está en el `<head>`: ejecutarlo de forma síncrona retrasaba el primer
//! montaje de la UI. Se precarga en segundo plano tras montar y la terminal y el
//! editor esperan a que esté listo.

use wasm_bindgen::prelude::*;

#[wasm_bindgen(inline_js = r#"
export function load_glue(src) {
  if (window.GryphusJs) return Promise.resolve();
  if (!window.__gryphusGlue) {
    window.__gryphusGlue = new Promise((resolve, reject) => {
      const script = document.createElement('script');
      script.src = src;
      script.onload = () => resolve();
      script.onerror = () => {
        window.__gryphusGlue = null;
        reject(new Error('no se pudo cargar ' + src));
      };
      document.head.appendChild(script);
    });
  }
  return window.__gryphusGlue;
}
"#)]
extern "C" {
    #[wasm_bindgen(catch)]
    async fn load_glue(src: &str) -> Result<JsValue, JsValue>;
}

/// Asegura que `window.GryphusJs` existe (lo carga una sola vez).
pub async fn ensure_loaded() -> Result<(), String> {
    load_glue("gryphus-js.js")
        .await
        .map(|_| ())
        .map_err(|e| e.as_string().unwrap_or_else(|| format!("{e:?}")))
}
