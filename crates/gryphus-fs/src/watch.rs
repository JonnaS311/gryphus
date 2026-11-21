//! Watcher recursivo con debounce. Un hilo agrupa los eventos de `notify` durante
//! `debounce` y entrega un lote ordenado y sin duplicados.

use std::collections::{BTreeSet, HashMap};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use gryphus_core::ports::{FsChangeSink, PortError, PortErrorKind, PortResult, Subscription};
use gryphus_core::RelPath;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use notify::{EventKind, RecursiveMode, Watcher};

use crate::TMP_MARKER;

pub(crate) fn start(
    root: &Path,
    debounce: Duration,
    sink: Arc<dyn FsChangeSink>,
) -> PortResult<Subscription> {
    if !root.is_dir() {
        return Err(PortError::new(
            PortErrorKind::NotFound,
            format!("no existe {}", root.display()),
        ));
    }
    let (tx, rx) = mpsc::channel::<PathBuf>();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(event) = res else { return };
        // Las lecturas no cambian nada. En Linux (inotify) llegan como `Access` y,
        // si no se descartan, leer `.git` (p. ej. `git worktree list` al recibir
        // un cambio de git) vuelve a disparar el watcher en un bucle sin fin.
        if matches!(event.kind, EventKind::Access(_)) {
            return;
        }
        let created = matches!(event.kind, EventKind::Create(_));
        for path in event.paths {
            // inotify vigila un directorio nuevo solo después de crearse: lo que
            // se escribió dentro en ese intervalo no genera eventos. Se recorre.
            if created && path.is_dir() {
                let mut budget = NEW_DIR_BUDGET;
                walk_new_dir(&path, &mut budget, &mut |p| {
                    let _ = tx.send(p);
                });
            }
            let _ = tx.send(path);
        }
    })
    .map_err(|e| PortError::new(PortErrorKind::Io, e.to_string()))?;
    watcher
        .watch(root, RecursiveMode::Recursive)
        .map_err(|e| PortError::new(PortErrorKind::Io, e.to_string()))?;

    let stopped = Arc::new(AtomicBool::new(false));
    let stop_flag = stopped.clone();
    let roots = [root.to_path_buf(), root.canonicalize().unwrap_or_default()];
    let mut ignores = IgnoreCache::new(root.to_path_buf());
    thread::Builder::new()
        .name("gryphus-fs-watch".into())
        .spawn(move || {
            while let Ok(first) = rx.recv() {
                let mut batch = BTreeSet::new();
                batch.extend(to_rel(&roots, &first));
                let deadline = Instant::now() + debounce;
                while let Some(left) = deadline.checked_duration_since(Instant::now()) {
                    match rx.recv_timeout(left) {
                        Ok(path) => batch.extend(to_rel(&roots, &path)),
                        Err(RecvTimeoutError::Timeout) => break,
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                }
                if stopped.load(Ordering::SeqCst) {
                    return;
                }
                if batch.iter().any(|p| p.as_str().ends_with(".gitignore")) {
                    ignores.clear();
                }
                let (ignored, relevant): (Vec<RelPath>, Vec<RelPath>) =
                    batch.into_iter().partition(|p| ignores.is_ignored(p));
                if !relevant.is_empty() {
                    sink.changed(relevant);
                }
                if !ignored.is_empty() {
                    sink.changed_ignored(ignored);
                }
            }
        })
        .map_err(|e| PortError::new(PortErrorKind::Io, e.to_string()))?;

    Ok(Subscription::new(move || {
        stop_flag.store(true, Ordering::SeqCst);
        drop(watcher); // cierra el canal: el hilo termina
    }))
}

/// Máximo de entradas que se reportan al recorrer un directorio recién creado
/// (un `npm install` puede crear cientos de miles; basta con el aviso del padre).
const NEW_DIR_BUDGET: usize = 2_000;

/// Reporta el contenido de un directorio recién creado, hasta `budget` entradas.
fn walk_new_dir(dir: &Path, budget: &mut usize, report: &mut dyn FnMut(PathBuf)) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if *budget == 0 {
            return;
        }
        *budget -= 1;
        let path = entry.path();
        let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
        report(path.clone());
        if is_dir {
            walk_new_dir(&path, budget, report);
        }
    }
}

/// Decide si una ruta está ignorada por git, con las mismas fuentes que
/// `list_dir`: `.gitignore` de cada directorio (el más profundo manda) y
/// `.git/info/exclude`. Los matchers se cargan perezosamente por directorio.
struct IgnoreCache {
    root: PathBuf,
    dirs: HashMap<PathBuf, Option<Gitignore>>,
    exclude: Option<Option<Gitignore>>,
}

impl IgnoreCache {
    fn new(root: PathBuf) -> Self {
        Self {
            root,
            dirs: HashMap::new(),
            exclude: None,
        }
    }

    fn clear(&mut self) {
        self.dirs.clear();
        self.exclude = None;
    }

    fn load(dir: &Path, file: &Path) -> Option<Gitignore> {
        if !file.is_file() {
            return None;
        }
        let mut builder = GitignoreBuilder::new(dir);
        builder.add(file);
        builder.build().ok().filter(|g| !g.is_empty())
    }

    fn is_ignored(&mut self, rel: &RelPath) -> bool {
        if rel.as_str() == ".git" {
            return false; // estado interno de git: siempre relevante
        }
        let full = rel.to_path(&self.root);
        let is_dir = full.is_dir();
        // Del directorio más profundo a la raíz: el primer patrón que decide manda.
        let mut dirs: Vec<PathBuf> = full
            .ancestors()
            .skip(1)
            .take_while(|d| d.starts_with(&self.root))
            .map(Path::to_path_buf)
            .collect();
        if dirs.is_empty() {
            dirs.push(self.root.clone());
        }
        for dir in dirs {
            let matcher = self
                .dirs
                .entry(dir.clone())
                .or_insert_with(|| Self::load(&dir, &dir.join(".gitignore")));
            if let Some(m) = matcher {
                let decision = m.matched_path_or_any_parents(&full, is_dir);
                if decision.is_ignore() {
                    return true;
                }
                if decision.is_whitelist() {
                    return false;
                }
            }
        }
        let root = self.root.clone();
        let exclude = self.exclude.get_or_insert_with(|| {
            Self::load(&root, &root.join(".git").join("info").join("exclude"))
        });
        exclude
            .as_ref()
            .is_some_and(|m| m.matched_path_or_any_parents(&full, is_dir).is_ignore())
    }
}

/// Ruta absoluta del evento → `RelPath`. Todo lo de `.git/` se resume en `.git`.
fn to_rel(roots: &[PathBuf], path: &Path) -> Option<RelPath> {
    let rel = roots.iter().find_map(|r| path.strip_prefix(r).ok())?;
    let mut parts = Vec::new();
    for comp in rel.components() {
        let Component::Normal(name) = comp else {
            return None;
        };
        let name = name.to_string_lossy();
        if parts.is_empty() && name == ".git" {
            return RelPath::parse(".git").ok();
        }
        if name.contains(TMP_MARKER) {
            return None;
        }
        parts.push(name.into_owned());
    }
    if parts.is_empty() {
        return None;
    }
    RelPath::parse(&parts.join("/")).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_event_paths_to_rel_paths() {
        let roots = [PathBuf::from("/r"), PathBuf::new()];
        let rel = |p: &str| to_rel(&roots, Path::new(p)).map(|r| r.as_str().to_owned());
        assert_eq!(rel("/r/src/a.rs").as_deref(), Some("src/a.rs"));
        assert_eq!(rel("/r/.git/index").as_deref(), Some(".git"));
        assert_eq!(rel("/r/.a.txt.gryphus-tmp-1-2"), None);
        assert_eq!(rel("/r"), None);
        assert_eq!(rel("/otro/x"), None);
    }
}
