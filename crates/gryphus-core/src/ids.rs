//! Identificadores tipados. Son newtypes para que el compilador impida mezclarlos.
//!
//! `FeatureId`, `WorktreeId` y `AgentId` son secuenciales y los asigna [`crate::Project`]
//! (deterministas, sin aleatoriedad ni IO). `ProjectId` se deriva de la ruta del repo.

use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};

macro_rules! seq_id {
    ($(#[$meta:meta])* $name:ident, $prefix:literal) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub(crate) u64);

        impl $name {
            /// Reconstruye un id recibido desde fuera (IPC, almacenamiento).
            /// Que exista en un proyecto lo valida [`crate::Project`] al usarlo.
            pub fn from_raw(raw: u64) -> Self {
                Self(raw)
            }

            pub fn get(self) -> u64 {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!($prefix, "-{}"), self.0)
            }
        }
    };
}

seq_id!(
    /// Identifica una feature dentro de un proyecto.
    FeatureId,
    "feat"
);
seq_id!(
    /// Identifica un worktree registrado en un proyecto.
    WorktreeId,
    "wt"
);
seq_id!(
    /// Identifica un agente dentro de un proyecto.
    AgentId,
    "agent"
);

/// Id estable de un proyecto, derivado de la ruta canónica de su repositorio.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProjectId(String);

impl ProjectId {
    /// La canonicalización de la ruta la hace el adaptador (IO); aquí solo se hashea.
    pub fn from_canonical_root(root: &Path) -> Self {
        let hash = fnv1a64(root.to_string_lossy().as_bytes());
        Self(format!("{hash:016x}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// FNV-1a de 64 bits: estable entre versiones de Rust (a diferencia de `DefaultHasher`).
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv1a64_matches_reference_vectors() {
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn project_id_is_stable_and_depends_on_path() {
        let a = ProjectId::from_canonical_root(Path::new("/repos/gryphus"));
        let b = ProjectId::from_canonical_root(Path::new("/repos/gryphus"));
        let c = ProjectId::from_canonical_root(Path::new("/repos/otro"));
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.as_str().len(), 16);
    }

    #[test]
    fn ids_display_with_prefix() {
        assert_eq!(FeatureId(3).to_string(), "feat-3");
        assert_eq!(WorktreeId(4).to_string(), "wt-4");
        assert_eq!(AgentId(5).to_string(), "agent-5");
    }

    #[test]
    fn ids_serialize_as_plain_numbers() {
        assert_eq!(serde_json::to_string(&AgentId(7)).unwrap(), "7");
    }
}
