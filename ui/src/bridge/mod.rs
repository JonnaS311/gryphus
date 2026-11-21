//! Único módulo de la UI que toca JavaScript.
//!
//! - [`tauri`]: `invoke`, `listen` (vía `window.__TAURI__`, `withGlobalTauri`).
//! - [`xterm`] y [`editor`]: envoltorios de `window.GryphusJs` (ui/js).
//! - [`dialog`]: confirmaciones nativas del navegador.
//! - [`icons`]: tablas de Material Icon Theme; [`prefs`]: localStorage y variables CSS.

pub mod dialog;
pub mod editor;
pub mod glue;
pub mod icons;
pub mod perf;
pub mod prefs;
pub mod tauri;
pub mod xterm;
