//! Editor CodeMirror 6 (ver ui/js/src/index.js → createEditor).

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    type JsEditor;

    #[wasm_bindgen(js_namespace = GryphusJs, js_name = createEditor)]
    fn create_editor(el: &JsValue, on_save: &JsValue, on_dirty: &JsValue) -> JsEditor;

    #[wasm_bindgen(method, js_name = setDoc)]
    fn set_doc(this: &JsEditor, text: &str, name: &str);
    #[wasm_bindgen(method, js_name = markSaved)]
    fn mark_saved(this: &JsEditor, text: &str);
    #[wasm_bindgen(method, js_name = getText)]
    fn get_text(this: &JsEditor) -> String;
    #[wasm_bindgen(method)]
    fn focus(this: &JsEditor);
    #[wasm_bindgen(method)]
    fn destroy(this: &JsEditor);
}

pub struct Editor {
    js: JsEditor,
    _on_save: Closure<dyn FnMut(String)>,
    _on_dirty: Closure<dyn FnMut(bool)>,
}

impl Editor {
    /// Carga el pegamento JS si hace falta y crea el editor.
    /// `on_save` se llama con Ctrl+S; `on_dirty` cuando cambia el estado "sin guardar".
    pub async fn create(
        el: &impl AsRef<JsValue>,
        on_save: impl FnMut(String) + 'static,
        on_dirty: impl FnMut(bool) + 'static,
    ) -> Result<Self, String> {
        super::glue::ensure_loaded().await?;
        Ok(Self::new(el, on_save, on_dirty))
    }

    fn new(
        el: &impl AsRef<JsValue>,
        on_save: impl FnMut(String) + 'static,
        on_dirty: impl FnMut(bool) + 'static,
    ) -> Self {
        let on_save = Closure::<dyn FnMut(String)>::new(on_save);
        let on_dirty = Closure::<dyn FnMut(bool)>::new(on_dirty);
        let js = create_editor(el.as_ref(), on_save.as_ref(), on_dirty.as_ref());
        Self {
            js,
            _on_save: on_save,
            _on_dirty: on_dirty,
        }
    }

    /// Carga otro archivo (reinicia el historial de deshacer).
    pub fn set_doc(&self, text: &str, name: &str) {
        self.js.set_doc(text, name);
    }

    pub fn mark_saved(&self, text: &str) {
        self.js.mark_saved(text);
    }

    pub fn text(&self) -> String {
        self.js.get_text()
    }

    pub fn focus(&self) {
        self.js.focus();
    }

    pub fn destroy(&self) {
        self.js.destroy();
    }
}
