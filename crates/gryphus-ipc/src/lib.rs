//! Contrato IPC compartido entre src-tauri y la UI (WASM): DTOs, nombres de comandos/eventos y errores. Solo depende de serde.
//!
//! Ver docs/architecture/overview.md.
#![forbid(unsafe_code)]

#[cfg(test)]
mod tests {
    #[test]
    fn crate_compiles() {
        assert_eq!(env!("CARGO_PKG_NAME"), "gryphus-ipc");
    }
}
