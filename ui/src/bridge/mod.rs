//! Único módulo de la UI que toca JavaScript.
//!
//! - [`tauri`]: `invoke`, `listen` (vía `window.__TAURI__`, `withGlobalTauri`).
//! - [`xterm`] y [`editor`]: envoltorios de `window.GryphusJs` (ui/js);
//!   [`highlight`]: colores de sintaxis del diff.
//! - [`dialog`]: confirmaciones nativas del navegador.
//! - [`icons`]: tablas de Material Icon Theme; [`prefs`]: localStorage y variables CSS.
//! - [`dom`]: desplazamiento a elementos (lista de archivos del diff) y
//!   modificador de los atajos (⌘ en macOS, Ctrl en el resto).

pub mod dialog;
pub mod dom;
pub mod editor;
pub mod glue;
pub mod highlight;
pub mod icons;
pub mod perf;
pub mod prefs;
pub mod tauri;
pub mod xterm;
