//! Fakes de todos los puertos, fixtures (repos git temporales) y suites de contrato.
//!
//! Ver docs/architecture/overview.md.
#![forbid(unsafe_code)]

#[cfg(test)]
mod tests {
    #[test]
    fn crate_compiles() {
        assert_eq!(env!("CARGO_PKG_NAME"), "gryphus-testkit");
    }
}
