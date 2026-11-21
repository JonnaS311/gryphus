//! Tablas de iconos (ui/icons/manifest.json, ~370 KB). Se descargan tras montar la
//! UI para no pesar en el arranque; hasta entonces el explorador usa los iconos por defecto.

use std::cell::RefCell;

use js_sys::{Object, Reflect};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

use crate::icons::Table;

thread_local! {
    static MANIFEST: RefCell<Option<JsValue>> = const { RefCell::new(None) };
}

fn js_err(e: JsValue) -> String {
    e.as_string().unwrap_or_else(|| format!("{e:?}"))
}

pub async fn load() -> Result<(), String> {
    let window = web_sys::window().ok_or("sin window")?;
    let response: web_sys::Response = JsFuture::from(window.fetch_with_str("icons/manifest.json"))
        .await
        .map_err(js_err)?
        .dyn_into()
        .map_err(js_err)?;
    if !response.ok() {
        return Err(format!("icons/manifest.json: HTTP {}", response.status()));
    }
    let json = JsFuture::from(response.json().map_err(js_err)?)
        .await
        .map_err(js_err)?;
    MANIFEST.with(|m| *m.borrow_mut() = Some(json));
    Ok(())
}

/// Busca `key` en una tabla del manifiesto (`None` si aún no se cargó).
pub fn get(table: Table, key: &str) -> Option<String> {
    MANIFEST.with(|m| {
        let m = m.borrow();
        let table = Reflect::get(m.as_ref()?, &JsValue::from_str(table.key())).ok()?;
        if table.is_undefined() {
            return None;
        }
        // Las tablas son objetos planos: evita heredar `constructor`, `toString`…
        let table: Object = table.unchecked_into();
        if !Object::has_own(&table, &JsValue::from_str(key)) {
            return None;
        }
        Reflect::get(&table, &JsValue::from_str(key))
            .ok()?
            .as_string()
    })
}
