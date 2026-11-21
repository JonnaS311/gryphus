//! Adaptador `FsPort` sobre el sistema de archivos local.
//!
//! - Toda ruta es `RelPath` (sin `..` ni absolutas) y además se comprueba que,
//!   tras resolver symlinks, sigue dentro del worktree.
//! - `list_dir` lee un solo nivel y marca lo ignorado por `.gitignore` (crate `ignore`).
//! - `write_file` es atómica: archivo temporal en el mismo directorio + `rename`.
//! - `watch` usa `notify` y entrega lotes con debounce; todo lo que ocurre dentro
//!   de `.git/` se resume en la ruta `.git`.
//!
//! Ver docs/architecture/overview.md y el contrato en gryphus-testkit.
#![forbid(unsafe_code)]

mod watch;

use std::collections::HashSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use gryphus_core::ports::{
    DirEntry, EntryKind, FileContent, FsChangeSink, FsPort, PortError, PortErrorKind, PortResult,
    Subscription,
};
use gryphus_core::RelPath;

/// Tamaño máximo que `read_file` devuelve como texto.
pub const DEFAULT_SIZE_LIMIT: u64 = 2 * 1024 * 1024;
/// Ventana de agrupación de eventos del watcher.
pub const DEFAULT_DEBOUNCE: Duration = Duration::from_millis(150);
/// Marca de los archivos temporales de escritura atómica (el watcher los ignora).
pub(crate) const TMP_MARKER: &str = ".gryphus-tmp-";

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub struct LocalFs {
    size_limit: u64,
    debounce: Duration,
}

impl Default for LocalFs {
    fn default() -> Self {
        Self::new()
    }
}

impl LocalFs {
    pub fn new() -> Self {
        Self {
            size_limit: DEFAULT_SIZE_LIMIT,
            debounce: DEFAULT_DEBOUNCE,
        }
    }

    pub fn with_limit(size_limit: u64) -> Self {
        Self {
            size_limit,
            ..Self::new()
        }
    }

    pub fn size_limit(&self) -> u64 {
        self.size_limit
    }
}

pub(crate) fn io_err(e: io::Error, what: &Path) -> PortError {
    let kind = match e.kind() {
        io::ErrorKind::NotFound => PortErrorKind::NotFound,
        io::ErrorKind::PermissionDenied => PortErrorKind::PermissionDenied,
        io::ErrorKind::AlreadyExists => PortErrorKind::AlreadyExists,
        _ => PortErrorKind::Io,
    };
    PortError::new(kind, format!("{}: {e}", what.display()))
}

/// Falla si `path` (ya existente), tras resolver symlinks, queda fuera de `root`.
fn ensure_inside(root: &Path, path: &Path) -> PortResult<()> {
    let canon_root = root.canonicalize().map_err(|e| io_err(e, root))?;
    let canon = path.canonicalize().map_err(|e| io_err(e, path))?;
    if canon.starts_with(&canon_root) {
        Ok(())
    } else {
        Err(PortError::new(
            PortErrorKind::PermissionDenied,
            format!("{} apunta fuera del worktree", path.display()),
        ))
    }
}

/// Ruta existente y contenida en el worktree.
fn resolve_existing(root: &Path, rel: &RelPath) -> PortResult<PathBuf> {
    let path = rel.to_path(root);
    fs::symlink_metadata(&path).map_err(|e| io_err(e, &path))?;
    ensure_inside(root, &path)?;
    Ok(path)
}

/// Nombres (de un nivel) que NO están ignorados por `.gitignore`.
fn not_ignored(dir: &Path) -> HashSet<String> {
    ignore::WalkBuilder::new(dir)
        .max_depth(Some(1))
        .hidden(false)
        .parents(true)
        .ignore(false)
        .git_ignore(true)
        .git_exclude(true)
        .git_global(false)
        .require_git(false)
        .build()
        .filter_map(Result::ok)
        .filter(|e| e.depth() == 1)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect()
}

impl FsPort for LocalFs {
    fn list_dir(&self, root: &Path, dir: &RelPath) -> PortResult<Vec<DirEntry>> {
        let path = resolve_existing(root, dir)?;
        if !path.is_dir() {
            return Err(PortError::new(
                PortErrorKind::InvalidInput,
                format!("{dir} no es un directorio"),
            ));
        }
        let visible = not_ignored(&path);
        let mut entries = Vec::new();
        for entry in fs::read_dir(&path).map_err(|e| io_err(e, &path))? {
            let entry = entry.map_err(|e| io_err(e, &path))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == ".git" || name.contains(TMP_MARKER) {
                continue;
            }
            let file_type = entry.file_type().map_err(|e| io_err(e, &entry.path()))?;
            let kind = if file_type.is_symlink() {
                EntryKind::Symlink
            } else if file_type.is_dir() {
                EntryKind::Dir
            } else {
                EntryKind::File
            };
            entries.push(DirEntry {
                ignored: !visible.contains(&name),
                name,
                kind,
            });
        }
        entries.sort_by_key(|e| (e.kind != EntryKind::Dir, e.name.to_lowercase()));
        Ok(entries)
    }

    fn read_file(&self, root: &Path, file: &RelPath) -> PortResult<FileContent> {
        if file.is_root() {
            return Err(PortError::new(
                PortErrorKind::InvalidInput,
                "la raíz es un directorio",
            ));
        }
        let path = resolve_existing(root, file)?;
        let meta = fs::metadata(&path).map_err(|e| io_err(e, &path))?;
        if meta.is_dir() {
            return Err(PortError::new(
                PortErrorKind::InvalidInput,
                format!("{file} es un directorio"),
            ));
        }
        let size = meta.len();
        if size > self.size_limit {
            return Ok(FileContent::TooLarge {
                size,
                limit: self.size_limit,
            });
        }
        let bytes = fs::read(&path).map_err(|e| io_err(e, &path))?;
        match String::from_utf8(bytes) {
            Ok(text) if !text.contains('\0') => Ok(FileContent::Text(text)),
            Ok(text) => Ok(FileContent::Binary {
                size: text.len() as u64,
            }),
            Err(e) => Ok(FileContent::Binary {
                size: e.as_bytes().len() as u64,
            }),
        }
    }

    fn write_file(&self, root: &Path, file: &RelPath, content: &[u8]) -> PortResult<()> {
        let Some(name) = file.components().last() else {
            return Err(PortError::new(
                PortErrorKind::InvalidInput,
                "la raíz no es un archivo",
            ));
        };
        let path = file.to_path(root);
        if path.is_dir() {
            return Err(PortError::new(
                PortErrorKind::InvalidInput,
                format!("{file} es un directorio"),
            ));
        }
        // El ancestro existente más cercano debe estar dentro del worktree.
        let existing = path
            .ancestors()
            .find(|a| a.exists())
            .unwrap_or(root)
            .to_path_buf();
        ensure_inside(root, &existing)?;

        let parent = path.parent().unwrap_or(root);
        fs::create_dir_all(parent).map_err(|e| io_err(e, parent))?;
        let tmp = parent.join(format!(
            ".{name}{TMP_MARKER}{}-{}",
            std::process::id(),
            TMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let result = (|| {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(content)?;
            f.sync_all()?;
            drop(f);
            fs::rename(&tmp, &path)
        })();
        if let Err(e) = result {
            let _ = fs::remove_file(&tmp);
            return Err(io_err(e, &path));
        }
        Ok(())
    }

    fn rename(&self, root: &Path, from: &RelPath, to: &RelPath) -> PortResult<()> {
        if from.is_root() || to.is_root() {
            return Err(root_err());
        }
        if to.starts_with(from) && to != from {
            return Err(into_itself(from));
        }
        let src = resolve_existing(root, from)?;
        let dst = prepare_target(root, to)?;
        // Cambiar solo mayúsculas (`a.txt` → `A.txt`) en Windows/macOS: `dst` "existe"
        // porque es el mismo archivo; eso no es un conflicto.
        if fs::symlink_metadata(&dst).is_ok() && !same_file(&src, &dst) {
            return Err(already_exists(to));
        }
        fs::rename(&src, &dst).map_err(|e| io_err(e, &dst))
    }

    fn copy(
        &self,
        from_root: &Path,
        from: &RelPath,
        to_root: &Path,
        to: &RelPath,
    ) -> PortResult<()> {
        if from.is_root() || to.is_root() {
            return Err(root_err());
        }
        let src = resolve_existing(from_root, from)?;
        let dst = prepare_target(to_root, to)?;
        if fs::symlink_metadata(&dst).is_ok() {
            return Err(already_exists(to));
        }
        if same_file(from_root, to_root) && to.starts_with(from) {
            return Err(into_itself(from));
        }
        copy_recursive(&src, &dst).inspect_err(|_| {
            let _ = if dst.is_dir() {
                fs::remove_dir_all(&dst)
            } else {
                fs::remove_file(&dst)
            };
        })
    }

    fn watch(&self, root: &Path, sink: Arc<dyn FsChangeSink>) -> PortResult<Subscription> {
        watch::start(root, self.debounce, sink)
    }
}

fn root_err() -> PortError {
    PortError::new(
        PortErrorKind::InvalidInput,
        "no se puede renombrar ni copiar la raíz del worktree",
    )
}

fn already_exists(rel: &RelPath) -> PortError {
    PortError::new(PortErrorKind::AlreadyExists, format!("{rel} ya existe"))
}

fn into_itself(rel: &RelPath) -> PortError {
    PortError::new(
        PortErrorKind::InvalidInput,
        format!("no se puede copiar ni mover {rel} dentro de sí mismo"),
    )
}

fn same_file(a: &Path, b: &Path) -> bool {
    matches!((a.canonicalize(), b.canonicalize()), (Ok(x), Ok(y)) if x == y)
}

/// Ruta destino: su directorio padre debe existir y estar dentro del worktree.
fn prepare_target(root: &Path, to: &RelPath) -> PortResult<PathBuf> {
    let parent = to.parent().unwrap_or_default();
    let parent_path = resolve_existing(root, &parent)?;
    if !parent_path.is_dir() {
        return Err(PortError::new(
            PortErrorKind::InvalidInput,
            format!("{parent} no es un directorio"),
        ));
    }
    Ok(to.to_path(root))
}

/// Copia archivos y directorios. Los symlinks internos se omiten (podrían apuntar
/// fuera del worktree) y `.git` nunca se copia.
fn copy_recursive(src: &Path, dst: &Path) -> PortResult<()> {
    let meta = fs::metadata(src).map_err(|e| io_err(e, src))?;
    if !meta.is_dir() {
        return fs::copy(src, dst).map(|_| ()).map_err(|e| io_err(e, dst));
    }
    fs::create_dir(dst).map_err(|e| io_err(e, dst))?;
    for entry in fs::read_dir(src).map_err(|e| io_err(e, src))? {
        let entry = entry.map_err(|e| io_err(e, src))?;
        let name = entry.file_name();
        let kind = entry.file_type().map_err(|e| io_err(e, &entry.path()))?;
        if kind.is_symlink() || name == ".git" || name.to_string_lossy().contains(TMP_MARKER) {
            continue;
        }
        copy_recursive(&entry.path(), &dst.join(&name))?;
    }
    Ok(())
}
