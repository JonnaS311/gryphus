//! UI de Gryphus (Leptos CSR, compilada a WASM con Trunk).
//!
//! Reglas (docs/architecture/ui.md):
//! - Solo `bridge/` usa wasm-bindgen / js-sys / web-sys (lo verifica tools/arch-guard).
//! - Solo `ipc` llama al bridge de Tauri.
//! - `logic` son funciones puras con pruebas nativas (`cargo test -p gryphus-ui`).

mod app;
mod bridge;
mod icons;
mod ipc;
mod layout;
mod logic;
mod panels;
mod settings;
mod store;
mod views;

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(app::App);
}
