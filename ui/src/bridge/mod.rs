//! Único módulo de la UI que toca JavaScript.
//!
//! - [`tauri`]: `invoke`, `listen` (vía `window.__TAURI__`, `withGlobalTauri`).
//! - [`xterm`] y [`editor`]: envoltorios de `window.GryphusJs` (ui/js).
//! - [`dialog`]: confirmaciones nativas del navegador.

pub mod dialog;
pub mod editor;
pub mod tauri;
pub mod xterm;
