//! Terminal xterm.js (ver ui/js/src/index.js → createTerminal).

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    type JsTerminal;

    #[wasm_bindgen(js_namespace = GryphusJs, js_name = createTerminal)]
    fn create_terminal(el: &JsValue, on_data: &JsValue, on_resize: &JsValue) -> JsTerminal;

    #[wasm_bindgen(method)]
    fn channel(this: &JsTerminal) -> JsValue;
    #[wasm_bindgen(method)]
    fn reset(this: &JsTerminal);
    #[wasm_bindgen(method)]
    fn fit(this: &JsTerminal);
    #[wasm_bindgen(method)]
    fn focus(this: &JsTerminal);
    #[wasm_bindgen(method)]
    fn write(this: &JsTerminal, text: &str);
    #[wasm_bindgen(method)]
    fn dispose(this: &JsTerminal);
}

/// Canal de Tauri cuya salida escribe directamente en la terminal (sin pasar por WASM).
pub struct TerminalChannel(JsValue);

impl TerminalChannel {
    pub(super) fn as_js(&self) -> &JsValue {
        &self.0
    }
}

pub struct Terminal {
    js: JsTerminal,
    _on_data: Closure<dyn FnMut(String)>,
    _on_resize: Closure<dyn FnMut(u16, u16)>,
}

impl Terminal {
    pub fn new(
        el: &impl AsRef<JsValue>,
        on_data: impl FnMut(String) + 'static,
        on_resize: impl FnMut(u16, u16) + 'static,
    ) -> Self {
        let on_data = Closure::<dyn FnMut(String)>::new(on_data);
        let on_resize = Closure::<dyn FnMut(u16, u16)>::new(on_resize);
        let js = create_terminal(el.as_ref(), on_data.as_ref(), on_resize.as_ref());
        Self {
            js,
            _on_data: on_data,
            _on_resize: on_resize,
        }
    }

    pub fn channel(&self) -> TerminalChannel {
        TerminalChannel(self.js.channel())
    }

    pub fn reset(&self) {
        self.js.reset();
    }

    /// Ajusta al contenedor e informa el tamaño (dispara `on_resize`).
    pub fn fit(&self) {
        self.js.fit();
    }

    pub fn focus(&self) {
        self.js.focus();
    }

    pub fn write(&self, text: &str) {
        self.js.write(text);
    }

    pub fn dispose(&self) {
        self.js.dispose();
    }
}
