//! Casos de uso de Gryphus. Orquesta los puertos de gryphus-core; nunca conoce adaptadores concretos.
//!
//! Ver docs/architecture/overview.md.
#![forbid(unsafe_code)]

#[cfg(test)]
mod tests {
    #[test]
    fn crate_compiles() {
        assert_eq!(env!("CARGO_PKG_NAME"), "gryphus-app");
    }
}
