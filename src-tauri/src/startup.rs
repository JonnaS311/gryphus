//! Medición del arranque en frío.
//!
//! - `GRYPHUS_STARTUP_LOG=<archivo>`: añade una línea `fase ms` por cada hito
//!   (ms desde el inicio de `main`).
//! - `GRYPHUS_STARTUP_EXIT=1`: sale en cuanto la UI está lista (para medir en bucle).

use std::io::Write;
use std::sync::OnceLock;
use std::time::Instant;

pub static PROCESS_START: OnceLock<Instant> = OnceLock::new();

pub fn begin() {
    let _ = PROCESS_START.set(Instant::now());
}

pub fn elapsed_ms() -> u64 {
    PROCESS_START
        .get()
        .map_or(0, |t| t.elapsed().as_millis() as u64)
}

pub fn trace(phase: &str) {
    let Some(path) = std::env::var_os("GRYPHUS_STARTUP_LOG") else {
        return;
    };
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(f, "{phase} {}", elapsed_ms());
    }
}

pub fn exit_when_ready() -> bool {
    std::env::var("GRYPHUS_STARTUP_EXIT").is_ok_and(|v| v == "1")
}
