//! [`FakeFs`]: árbol de archivos en memoria con un solo worktree raíz.
//!
//! `.gitignore` simplificado: solo nombres exactos (`secret.txt`) y directorios
//! (`target/`), aplicados en cualquier nivel. Suficiente para el contrato.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use gryphus_core::ports::{
    DirEntry, EntryKind, FileContent, FsChangeSink, FsPort, PortError, PortErrorKind, PortResult,
    Subscription,
};
use gryphus_core::RelPath;

use super::{err, lock, FailSwitch};

pub const DEFAULT_SIZE_LIMIT: u64 = 1024 * 1024;

enum Node {
    File(Vec<u8>),
    Dir,
}

type Watchers = Vec<(u64, Arc<dyn FsChangeSink>)>;

struct Inner {
    root: PathBuf,
    size_limit: u64,
    /// Clave: `RelPath::as_str()` (la raíz es implícita).
    nodes: Mutex<BTreeMap<String, Node>>,
    watchers: Mutex<Watchers>,
    next_watch: AtomicU64,
    fail: FailSwitch,
}

#[derive(Clone)]
pub struct FakeFs {
    inner: Arc<Inner>,
}

impl FakeFs {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self::with_limit(root, DEFAULT_SIZE_LIMIT)
    }

    pub fn with_limit(root: impl Into<PathBuf>, size_limit: u64) -> Self {
        Self {
            inner: Arc::new(Inner {
                root: root.into(),
                size_limit,
                nodes: Mutex::new(BTreeMap::new()),
                watchers: Mutex::new(Vec::new()),
                next_watch: AtomicU64::new(0),
                fail: FailSwitch::default(),
            }),
        }
    }

    pub fn root(&self) -> PathBuf {
        self.inner.root.clone()
    }

    pub fn size_limit(&self) -> u64 {
        self.inner.size_limit
    }

    pub fn fail_next(&self, error: PortError) {
        self.inner.fail.set(error);
    }

    /// Simula un cambio externo (un agente escribe): crea padres y avisa a los watchers.
    pub fn write_raw(&self, rel: &str, bytes: &[u8]) {
        let rel = RelPath::parse(rel).expect("ruta relativa válida");
        self.insert_file(&rel, bytes).expect("escritura en FakeFs");
    }

    pub fn create_dir(&self, rel: &str) {
        let rel = RelPath::parse(rel).expect("ruta relativa válida");
        let mut nodes = lock(&self.inner.nodes);
        for prefix in prefixes(&rel) {
            nodes.entry(prefix).or_insert(Node::Dir);
        }
    }

    fn check_root(&self, root: &Path) -> PortResult<()> {
        if root == self.inner.root {
            Ok(())
        } else {
            err(
                PortErrorKind::NotFound,
                format!("raíz desconocida: {}", root.display()),
            )
        }
    }

    fn insert_file(&self, rel: &RelPath, bytes: &[u8]) -> PortResult<()> {
        if rel.is_root() {
            return err(PortErrorKind::InvalidInput, "la raíz no es un archivo");
        }
        {
            let mut nodes = lock(&self.inner.nodes);
            let all = prefixes(rel);
            let (file, parents) = all.split_last().expect("al menos un componente");
            for parent in parents {
                match nodes.get(parent) {
                    Some(Node::File(_)) => {
                        return err(
                            PortErrorKind::InvalidInput,
                            format!("{parent} es un archivo"),
                        );
                    }
                    Some(Node::Dir) => {}
                    None => {
                        nodes.insert(parent.clone(), Node::Dir);
                    }
                }
            }
            if let Some(Node::Dir) = nodes.get(file) {
                return err(
                    PortErrorKind::InvalidInput,
                    format!("{file} es un directorio"),
                );
            }
            nodes.insert(file.clone(), Node::File(bytes.to_vec()));
        }
        self.notify(vec![rel.clone()]);
        Ok(())
    }

    /// Copia (`remove = false`) o mueve el nodo `from` y todo lo que cuelga de él.
    fn transfer(&self, from: &RelPath, to: &RelPath, remove: bool) -> PortResult<()> {
        if from.is_root() || to.is_root() {
            return err(PortErrorKind::InvalidInput, "la raíz no se puede mover");
        }
        if to.starts_with(from) && to != from {
            return err(
                PortErrorKind::InvalidInput,
                format!("{from} no se puede copiar ni mover dentro de sí mismo"),
            );
        }
        {
            let mut nodes = lock(&self.inner.nodes);
            if !nodes.contains_key(from.as_str()) {
                return err(PortErrorKind::NotFound, format!("{from} no existe"));
            }
            let parent = to.parent().unwrap_or_default();
            if !parent.is_root() && !matches!(nodes.get(parent.as_str()), Some(Node::Dir)) {
                return err(PortErrorKind::NotFound, format!("{parent} no existe"));
            }
            if to == from && remove {
                return Ok(());
            }
            if nodes.contains_key(to.as_str()) {
                return err(PortErrorKind::AlreadyExists, format!("{to} ya existe"));
            }
            let moved: Vec<String> = nodes
                .keys()
                .filter(|k| RelPath::parse(k).is_ok_and(|k| k.starts_with(from)))
                .cloned()
                .collect();
            for key in moved {
                let rest = &key[from.as_str().len()..];
                let target = format!("{}{rest}", to.as_str());
                let node = if remove {
                    nodes.remove(&key).expect("clave listada")
                } else {
                    match &nodes[&key] {
                        Node::Dir => Node::Dir,
                        Node::File(b) => Node::File(b.clone()),
                    }
                };
                nodes.insert(target, node);
            }
        }
        let mut changed = vec![to.clone()];
        if remove {
            changed.push(from.clone());
        }
        self.notify(changed);
        Ok(())
    }

    fn notify(&self, paths: Vec<RelPath>) {
        let watchers: Vec<_> = lock(&self.inner.watchers)
            .iter()
            .map(|(_, sink)| sink.clone())
            .collect();
        for sink in watchers {
            sink.changed(paths.clone());
        }
    }

    fn ignore_patterns(nodes: &BTreeMap<String, Node>) -> Vec<String> {
        match nodes.get(".gitignore") {
            Some(Node::File(bytes)) => String::from_utf8_lossy(bytes)
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .map(str::to_owned)
                .collect(),
            _ => Vec::new(),
        }
    }
}

/// `a/b/c` → `["a", "a/b", "a/b/c"]`.
fn prefixes(rel: &RelPath) -> Vec<String> {
    let mut acc = String::new();
    rel.components()
        .map(|c| {
            if !acc.is_empty() {
                acc.push('/');
            }
            acc.push_str(c);
            acc.clone()
        })
        .collect()
}

impl FsPort for FakeFs {
    fn list_dir(&self, root: &Path, dir: &RelPath) -> PortResult<Vec<DirEntry>> {
        self.inner.fail.check()?;
        self.check_root(root)?;
        let nodes = lock(&self.inner.nodes);
        let key = dir.as_str();
        if !dir.is_root() {
            match nodes.get(key) {
                None => return err(PortErrorKind::NotFound, format!("{key} no existe")),
                Some(Node::File(_)) => {
                    return err(
                        PortErrorKind::InvalidInput,
                        format!("{key} no es un directorio"),
                    )
                }
                Some(Node::Dir) => {}
            }
        }
        let prefix = if dir.is_root() {
            String::new()
        } else {
            format!("{key}/")
        };
        let patterns = Self::ignore_patterns(&nodes);
        let mut entries: Vec<DirEntry> = nodes
            .iter()
            .filter_map(|(path, node)| {
                let name = path.strip_prefix(&prefix)?;
                if name.is_empty() || name.contains('/') || name == ".git" {
                    return None;
                }
                let kind = match node {
                    Node::Dir => EntryKind::Dir,
                    Node::File(_) => EntryKind::File,
                };
                let ignored = patterns.iter().any(|p| {
                    p == name || (kind == EntryKind::Dir && p.strip_suffix('/') == Some(name))
                });
                Some(DirEntry {
                    name: name.to_owned(),
                    kind,
                    ignored,
                })
            })
            .collect();
        entries.sort_by_key(|e| (e.kind != EntryKind::Dir, e.name.to_lowercase()));
        Ok(entries)
    }

    fn read_file(&self, root: &Path, file: &RelPath) -> PortResult<FileContent> {
        self.inner.fail.check()?;
        self.check_root(root)?;
        let nodes = lock(&self.inner.nodes);
        match nodes.get(file.as_str()) {
            None if !file.is_root() => err(PortErrorKind::NotFound, format!("{file} no existe")),
            None | Some(Node::Dir) => err(
                PortErrorKind::InvalidInput,
                format!("{file} es un directorio"),
            ),
            Some(Node::File(bytes)) => {
                let size = bytes.len() as u64;
                if size > self.inner.size_limit {
                    return Ok(FileContent::TooLarge {
                        size,
                        limit: self.inner.size_limit,
                    });
                }
                match std::str::from_utf8(bytes) {
                    Ok(text) if !text.contains('\0') => Ok(FileContent::Text(text.to_owned())),
                    _ => Ok(FileContent::Binary { size }),
                }
            }
        }
    }

    fn write_file(&self, root: &Path, file: &RelPath, content: &[u8]) -> PortResult<()> {
        self.inner.fail.check()?;
        self.check_root(root)?;
        self.insert_file(file, content)
    }

    fn rename(&self, root: &Path, from: &RelPath, to: &RelPath) -> PortResult<()> {
        self.inner.fail.check()?;
        self.check_root(root)?;
        self.transfer(from, to, true)
    }

    fn copy(
        &self,
        from_root: &Path,
        from: &RelPath,
        to_root: &Path,
        to: &RelPath,
    ) -> PortResult<()> {
        self.inner.fail.check()?;
        self.check_root(from_root)?;
        self.check_root(to_root)?;
        self.transfer(from, to, false)
    }

    fn watch(&self, root: &Path, sink: Arc<dyn FsChangeSink>) -> PortResult<Subscription> {
        self.inner.fail.check()?;
        self.check_root(root)?;
        let id = self.inner.next_watch.fetch_add(1, Ordering::SeqCst);
        lock(&self.inner.watchers).push((id, sink));
        let inner = self.inner.clone();
        Ok(Subscription::new(move || {
            lock(&inner.watchers).retain(|(w, _)| *w != id);
        }))
    }
}
