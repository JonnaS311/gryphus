//! Editor CodeMirror 6 con varios documentos (ver ui/js/src/index.js → createEditor).
//! Cada documento se identifica con una clave de texto y conserva su historial.

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    type JsEditor;

    #[wasm_bindgen(js_namespace = GryphusJs, js_name = createEditor)]
    fn create_editor(el: &JsValue, on_save: &JsValue, on_dirty: &JsValue) -> JsEditor;

    #[wasm_bindgen(method, js_name = hasDoc)]
    fn has_doc(this: &JsEditor, key: &str) -> bool;
    #[wasm_bindgen(method, js_name = openDoc)]
    fn open_doc(this: &JsEditor, key: &str, text: &str, name: &str);
    #[wasm_bindgen(method, js_name = showDoc)]
    fn show_doc(this: &JsEditor, key: &str);
    #[wasm_bindgen(method, js_name = reloadDoc)]
    fn reload_doc(this: &JsEditor, key: &str, text: &str);
    #[wasm_bindgen(method, js_name = closeDoc)]
    fn close_doc(this: &JsEditor, key: &str);
    #[wasm_bindgen(method)]
    fn clear(this: &JsEditor);
    #[wasm_bindgen(method, js_name = markSaved)]
    fn mark_saved(this: &JsEditor, key: &str, text: &str);
    #[wasm_bindgen(method, js_name = getText)]
    fn get_text(this: &JsEditor, key: &str) -> Option<String>;
    #[wasm_bindgen(method)]
    fn focus(this: &JsEditor);
    #[wasm_bindgen(method)]
    fn destroy(this: &JsEditor);
}

pub struct Editor {
    js: JsEditor,
    _on_save: Closure<dyn FnMut(String)>,
    _on_dirty: Closure<dyn FnMut(String, bool)>,
}

impl Editor {
    /// Carga el pegamento JS si hace falta y crea el editor.
    /// `on_save` se llama con Ctrl+S (texto del documento activo); `on_dirty(clave, sucio)`
    /// cuando un documento pasa a tener o no cambios sin guardar.
    pub async fn create(
        el: &impl AsRef<JsValue>,
        on_save: impl FnMut(String) + 'static,
        on_dirty: impl FnMut(String, bool) + 'static,
    ) -> Result<Self, String> {
        super::glue::ensure_loaded().await?;
        Ok(Self::new(el, on_save, on_dirty))
    }

    fn new(
        el: &impl AsRef<JsValue>,
        on_save: impl FnMut(String) + 'static,
        on_dirty: impl FnMut(String, bool) + 'static,
    ) -> Self {
        let on_save = Closure::<dyn FnMut(String)>::new(on_save);
        let on_dirty = Closure::<dyn FnMut(String, bool)>::new(on_dirty);
        let js = create_editor(el.as_ref(), on_save.as_ref(), on_dirty.as_ref());
        Self {
            js,
            _on_save: on_save,
            _on_dirty: on_dirty,
        }
    }

    pub fn has_doc(&self, key: &str) -> bool {
        self.js.has_doc(key)
    }

    /// Abre (o reemplaza) un documento con texto de disco y lo muestra.
    pub fn open_doc(&self, key: &str, text: &str, name: &str) {
        self.js.open_doc(key, text, name);
    }

    /// Muestra un documento ya abierto (conserva cursor e historial).
    pub fn show_doc(&self, key: &str) {
        self.js.show_doc(key);
    }

    /// Reemplaza el texto de un documento sin cambios (cambió en disco).
    pub fn reload_doc(&self, key: &str, text: &str) {
        self.js.reload_doc(key, text);
    }

    pub fn close_doc(&self, key: &str) {
        self.js.close_doc(key);
    }

    /// Deja la vista sin documento activo.
    pub fn clear(&self) {
        self.js.clear();
    }

    pub fn mark_saved(&self, key: &str, text: &str) {
        self.js.mark_saved(key, text);
    }

    pub fn text(&self, key: &str) -> Option<String> {
        self.js.get_text(key)
    }

    pub fn focus(&self) {
        self.js.focus();
    }

    pub fn destroy(&self) {
        self.js.destroy();
    }
}
