//! Watcher recursivo con debounce. Un hilo agrupa los eventos de `notify` durante
//! `debounce` y entrega un lote ordenado y sin duplicados.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use gryphus_core::ports::{FsChangeSink, PortError, PortErrorKind, PortResult, Subscription};
use gryphus_core::RelPath;
use notify::{RecursiveMode, Watcher};

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
        if let Ok(event) = res {
            for path in event.paths {
                let _ = tx.send(path);
            }
        }
    })
    .map_err(|e| PortError::new(PortErrorKind::Io, e.to_string()))?;
    watcher
        .watch(root, RecursiveMode::Recursive)
        .map_err(|e| PortError::new(PortErrorKind::Io, e.to_string()))?;

    let stopped = Arc::new(AtomicBool::new(false));
    let stop_flag = stopped.clone();
    let roots = [root.to_path_buf(), root.canonicalize().unwrap_or_default()];
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
                if !batch.is_empty() {
                    sink.changed(batch.into_iter().collect());
                }
            }
        })
        .map_err(|e| PortError::new(PortErrorKind::Io, e.to_string()))?;

    Ok(Subscription::new(move || {
        stop_flag.store(true, Ordering::SeqCst);
        drop(watcher); // cierra el canal: el hilo termina
    }))
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
