//! [`RelPath`]: ruta relativa a la raíz de un worktree que no puede salir de ella.
//!
//! Toda ruta que llega desde la UI se convierte a `RelPath` antes de tocar disco.
//! Esto bloquea `..`, rutas absolutas, unidades de Windows (`C:`) y streams NTFS (`a:b`).
//! Los symlinks que apuntan fuera los bloquea el adaptador de fs (requiere IO).

use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PathError {
    #[error("la ruta debe ser relativa al worktree")]
    Absolute,
    #[error("la ruta no puede contener '..'")]
    ParentComponent,
    #[error("componente de ruta no permitido: {0:?}")]
    InvalidComponent(String),
}

/// Componentes separados por `/`, sin vacíos, `.` ni `..`. La cadena vacía es la raíz.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct RelPath(String);

impl RelPath {
    pub fn root() -> Self {
        Self(String::new())
    }

    pub fn parse(raw: &str) -> Result<Self, PathError> {
        if raw.starts_with('/') || raw.starts_with('\\') {
            return Err(PathError::Absolute);
        }
        let mut parts = Vec::new();
        for comp in raw.split(['/', '\\']) {
            match comp {
                "" | "." => continue,
                ".." => return Err(PathError::ParentComponent),
                c if c.contains(':') || c.contains('\0') || c.chars().all(|ch| ch == '.') => {
                    return Err(PathError::InvalidComponent(c.to_owned()));
                }
                c => parts.push(c),
            }
        }
        Ok(Self(parts.join("/")))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_root(&self) -> bool {
        self.0.is_empty()
    }

    pub fn components(&self) -> impl Iterator<Item = &str> {
        self.0.split('/').filter(|c| !c.is_empty())
    }

    pub fn join(&self, name: &str) -> Result<Self, PathError> {
        Self::parse(&format!("{}/{}", self.0, name))
    }

    /// Ruta absoluta bajo `root`. Siempre queda dentro de `root` (sin contar symlinks).
    pub fn to_path(&self, root: &Path) -> PathBuf {
        let mut path = root.to_path_buf();
        for comp in self.components() {
            path.push(comp);
        }
        path
    }
}

impl TryFrom<String> for RelPath {
    type Error = PathError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<RelPath> for String {
    fn from(value: RelPath) -> Self {
        value.0
    }
}

impl fmt::Display for RelPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn normalizes_separators_and_dots() {
        assert_eq!(
            RelPath::parse("src\\main.rs").unwrap().as_str(),
            "src/main.rs"
        );
        assert_eq!(
            RelPath::parse("./src//lib.rs/").unwrap().as_str(),
            "src/lib.rs"
        );
        assert!(RelPath::parse("").unwrap().is_root());
        assert!(RelPath::parse(".").unwrap().is_root());
    }

    #[test]
    fn rejects_escapes() {
        assert_eq!(RelPath::parse("../x"), Err(PathError::ParentComponent));
        assert_eq!(RelPath::parse("a/../../x"), Err(PathError::ParentComponent));
        assert_eq!(RelPath::parse("/etc/passwd"), Err(PathError::Absolute));
        assert_eq!(
            RelPath::parse("\\\\server\\share"),
            Err(PathError::Absolute)
        );
        assert!(matches!(
            RelPath::parse("C:\\Windows"),
            Err(PathError::InvalidComponent(_))
        ));
        assert!(matches!(
            RelPath::parse("C:foo"),
            Err(PathError::InvalidComponent(_))
        ));
        assert!(matches!(
            RelPath::parse("file.txt:stream"),
            Err(PathError::InvalidComponent(_))
        ));
        assert!(matches!(
            RelPath::parse("a/.../b"),
            Err(PathError::InvalidComponent(_))
        ));
    }

    #[test]
    fn join_and_to_path() {
        let dir = RelPath::parse("src").unwrap();
        let file = dir.join("lib.rs").unwrap();
        assert_eq!(file.as_str(), "src/lib.rs");
        assert!(dir.join("..").is_err());
        let root = Path::new("repo");
        assert_eq!(
            file.to_path(root),
            Path::new("repo").join("src").join("lib.rs")
        );
        assert_eq!(RelPath::root().to_path(root), root);
    }

    proptest! {
        /// Cualquier RelPath válida resuelve a una ruta dentro de la raíz.
        #[test]
        fn parsed_paths_never_escape_root(raw in "[a-z./\\\\:]{0,30}") {
            if let Ok(rel) = RelPath::parse(&raw) {
                let root = Path::new("root");
                let full = rel.to_path(root);
                prop_assert!(full.starts_with(root));
                prop_assert!(rel.components().all(|c| c != ".." && c != "." && !c.contains(':')));
                // Idempotente: volver a parsear da lo mismo.
                prop_assert_eq!(RelPath::parse(rel.as_str()), Ok(rel.clone()));
            }
        }
    }
}
