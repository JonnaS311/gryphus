//! Puerto de sistema de archivos. Implementación real en `gryphus-fs`.
//! Todas las rutas son [`RelPath`] relativas a la raíz de un worktree.

use std::path::Path;
use std::sync::Arc;

use crate::path::RelPath;

use super::{PortResult, Subscription};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Dir,
    Symlink,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    pub kind: EntryKind,
    /// Ignorado por `.gitignore` (la UI lo muestra atenuado).
    pub ignored: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileContent {
    Text(String),
    Binary { size: u64 },
    TooLarge { size: u64, limit: u64 },
}

/// Recibe lotes de rutas cambiadas (ya con debounce).
pub trait FsChangeSink: Send + Sync {
    fn changed(&self, paths: Vec<RelPath>);
}

pub trait FsPort: Send + Sync {
    /// Un solo nivel, ordenado: directorios primero, luego por nombre.
    fn list_dir(&self, root: &Path, dir: &RelPath) -> PortResult<Vec<DirEntry>>;
    fn read_file(&self, root: &Path, file: &RelPath) -> PortResult<FileContent>;
    /// Escritura atómica (archivo temporal + rename).
    fn write_file(&self, root: &Path, file: &RelPath, content: &[u8]) -> PortResult<()>;
    fn watch(&self, root: &Path, sink: Arc<dyn FsChangeSink>) -> PortResult<Subscription>;
}
