//! Traduce los cambios en disco de un worktree a eventos de dominio.

use std::sync::Weak;

use gryphus_core::ports::FsChangeSink;
use gryphus_core::{DomainEvent, RelPath, WorktreeId};

use crate::session::Inner;

pub(crate) struct WatchSink {
    pub(crate) session: Weak<Inner>,
    pub(crate) worktree: WorktreeId,
    pub(crate) is_main: bool,
}

impl FsChangeSink for WatchSink {
    fn changed(&self, paths: Vec<RelPath>) {
        let Some(inner) = self.session.upgrade() else {
            return;
        };
        let git_internal = paths.iter().any(|p| p.as_str() == ".git");
        let files: Vec<RelPath> = paths.into_iter().filter(|p| p.as_str() != ".git").collect();
        if !files.is_empty() {
            inner.publish(DomainEvent::FilesChanged {
                worktree: self.worktree,
                paths: files,
            });
        }
        inner.publish(DomainEvent::GitChanged {
            worktree: self.worktree,
        });
        // El `.git` del principal guarda el estado de todos los worktrees.
        if git_internal && self.is_main {
            let _ = inner.refresh_worktrees();
            inner.publish_git_changed_for_all();
        }
    }

    /// Rutas ignoradas por git: el explorador y el editor se actualizan, pero el
    /// estado git no cambió (antes cada lote de `target/` relanzaba `git status`).
    fn changed_ignored(&self, paths: Vec<RelPath>) {
        let Some(inner) = self.session.upgrade() else {
            return;
        };
        inner.publish(DomainEvent::FilesChanged {
            worktree: self.worktree,
            paths,
        });
    }
}
