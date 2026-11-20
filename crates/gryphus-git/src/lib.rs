//! Adaptador GitPort: gix para lecturas, CLI git para mutaciones.
//!
//! Ver docs/architecture/overview.md.
#![forbid(unsafe_code)]

#[cfg(test)]
mod tests {
    #[test]
    fn crate_compiles() {
        assert_eq!(env!("CARGO_PKG_NAME"), "gryphus-git");
    }
}
