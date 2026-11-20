//! Dominio de Gryphus: entidades, invariantes, errores y puertos (traits). Sin IO.
//!
//! Ver docs/architecture/overview.md.
#![forbid(unsafe_code)]

#[cfg(test)]
mod tests {
    #[test]
    fn crate_compiles() {
        assert_eq!(env!("CARGO_PKG_NAME"), "gryphus-core");
    }
}
