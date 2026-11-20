//! Punto de entrada de la UI (Leptos CSR, compilada a WASM con Trunk).
//!
//! Reglas (docs/architecture/ui.md):
//! - Solo `bridge/` usa wasm-bindgen / js-sys / web-sys (lo verifica tools/arch-guard).
//! - Solo `ipc/` llama al bridge de Tauri.

mod app;
mod layout;

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(app::App);
}
