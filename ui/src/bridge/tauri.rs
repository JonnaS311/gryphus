//! `invoke` y `listen` de Tauri con tipos de `gryphus-ipc`.

use gryphus_ipc::IpcError;
use js_sys::{Object, Reflect};
use serde::de::DeserializeOwned;
use serde::Serialize;
use wasm_bindgen::prelude::*;

use super::xterm::TerminalChannel;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(catch, js_namespace = ["__TAURI__", "core"], js_name = invoke)]
    async fn tauri_invoke(cmd: &str, args: JsValue) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(js_namespace = ["__TAURI__", "event"], js_name = listen)]
    fn tauri_listen(event: &str, handler: &Closure<dyn FnMut(JsValue)>) -> js_sys::Promise;
}

fn internal(message: impl Into<String>) -> IpcError {
    IpcError {
        code: "INTERNAL".into(),
        message: message.into(),
    }
}

fn to_js<T: Serialize>(value: &T) -> Result<JsValue, IpcError> {
    value
        .serialize(&serde_wasm_bindgen::Serializer::json_compatible())
        .map_err(|e| internal(e.to_string()))
}

async fn call<Res: DeserializeOwned>(cmd: &str, args: Object) -> Result<Res, IpcError> {
    match tauri_invoke(cmd, args.into()).await {
        Ok(value) => serde_wasm_bindgen::from_value(value).map_err(|e| internal(e.to_string())),
        Err(err) => Err(serde_wasm_bindgen::from_value::<IpcError>(err.clone())
            .unwrap_or_else(|_| internal(err.as_string().unwrap_or_else(|| format!("{err:?}"))))),
    }
}

fn args_with_req<Req: Serialize>(req: Option<&Req>) -> Result<Object, IpcError> {
    let args = Object::new();
    if let Some(req) = req {
        Reflect::set(&args, &JsValue::from_str("req"), &to_js(req)?)
            .map_err(|_| internal("no se pudieron preparar los argumentos"))?;
    }
    Ok(args)
}

/// Llama a un comando con un único argumento `req` (o ninguno).
pub async fn invoke<Req: Serialize, Res: DeserializeOwned>(
    cmd: &str,
    req: Option<&Req>,
) -> Result<Res, IpcError> {
    call(cmd, args_with_req(req)?).await
}

/// Igual que [`invoke`], pasando además el canal de salida de una terminal.
pub async fn invoke_with_channel<Req: Serialize, Res: DeserializeOwned>(
    cmd: &str,
    req: &Req,
    channel: &TerminalChannel,
) -> Result<Res, IpcError> {
    let args = args_with_req(Some(req))?;
    Reflect::set(&args, &JsValue::from_str("channel"), channel.as_js())
        .map_err(|_| internal("no se pudo adjuntar el canal"))?;
    call(cmd, args).await
}

/// Escucha un evento de Rust durante toda la vida de la ventana.
pub fn listen<T: DeserializeOwned + 'static>(event: &str, mut handler: impl FnMut(T) + 'static) {
    let callback = Closure::<dyn FnMut(JsValue)>::new(move |event: JsValue| {
        let payload = Reflect::get(&event, &JsValue::from_str("payload")).unwrap_or(JsValue::NULL);
        if let Ok(value) = serde_wasm_bindgen::from_value::<T>(payload) {
            handler(value);
        }
    });
    let _ = tauri_listen(event, &callback);
    callback.forget();
}
