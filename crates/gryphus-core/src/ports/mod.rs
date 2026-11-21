//! Puertos: lo único que la aplicación sabe del mundo exterior.
//!
//! Son traits **síncronos y sin runtime async**: `gryphus-app` decide en qué hilo
//! llamarlos (p. ej. `spawn_blocking`). Los flujos (salida PTY, cambios en disco)
//! se entregan por callbacks (`*Sink`) y se cancelan soltando una [`Subscription`].

pub mod agents;
pub mod fs;
pub mod git;
pub mod store;

pub use agents::{
    AgentChunk, AgentExit, AgentRuntimePort, ChunkSink, ExitCallback, SpawnSpec, TermSize,
};
pub use fs::{DirEntry, EntryKind, FileContent, FsChangeSink, FsPort};
pub use git::{
    Branch, Diff, DiffBase, DiffLine, FileDiff, FileState, FileStatus, GitPort, Hunk, NewWorktree,
    RepoInfo, WorktreeInfo,
};
pub use store::StorePort;

use std::fmt;

/// Error de un adaptador. `kind` permite a la app decidir sin mirar el texto.
/// Se muestra solo el mensaje: la UI ya añade el código estable (`CONFLICT`), y el
/// prefijo `Conflict: ` duplicaba la información en cada aviso.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct PortError {
    pub kind: PortErrorKind,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortErrorKind {
    NotFound,
    AlreadyExists,
    /// Conflicto de estado (p. ej. worktree con cambios sin commit).
    Conflict,
    InvalidInput,
    PermissionDenied,
    /// Herramienta externa ausente (p. ej. `git` o el CLI del agente).
    Unavailable,
    /// Datos persistidos ilegibles o que violan las invariantes.
    Corrupted,
    Io,
}

impl PortError {
    pub fn new(kind: PortErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn code(&self) -> &'static str {
        match self.kind {
            PortErrorKind::NotFound => "NOT_FOUND",
            PortErrorKind::AlreadyExists => "ALREADY_EXISTS",
            PortErrorKind::Conflict => "CONFLICT",
            PortErrorKind::InvalidInput => "INVALID_INPUT",
            PortErrorKind::PermissionDenied => "PERMISSION_DENIED",
            PortErrorKind::Unavailable => "UNAVAILABLE",
            PortErrorKind::Corrupted => "CORRUPTED",
            PortErrorKind::Io => "IO",
        }
    }
}

pub type PortResult<T> = Result<T, PortError>;

/// Suscripción activa a un flujo. Soltarla (`drop`) la cancela.
#[must_use = "soltar la Subscription cancela la suscripción inmediatamente"]
pub struct Subscription(Option<Box<dyn FnOnce() + Send>>);

impl Subscription {
    pub fn new(cancel: impl FnOnce() + Send + 'static) -> Self {
        Self(Some(Box::new(cancel)))
    }

    /// Para fakes o flujos que no necesitan limpieza.
    pub fn noop() -> Self {
        Self(None)
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        if let Some(cancel) = self.0.take() {
            cancel();
        }
    }
}

impl fmt::Debug for Subscription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Subscription")
            .field("active", &self.0.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    #[test]
    fn dropping_a_subscription_cancels_it() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = cancelled.clone();
        let sub = Subscription::new(move || flag.store(true, Ordering::SeqCst));
        assert!(!cancelled.load(Ordering::SeqCst));
        drop(sub);
        assert!(cancelled.load(Ordering::SeqCst));
    }

    /// Los puertos deben poder usarse como `dyn Trait` (inyección en `gryphus-app`).
    #[test]
    fn ports_are_object_safe() {
        fn accepts(
            _: Option<&dyn GitPort>,
            _: Option<&dyn AgentRuntimePort>,
            _: Option<&dyn FsPort>,
            _: Option<&dyn StorePort>,
            _: Option<&dyn crate::EventSink>,
        ) {
        }
        accepts(None, None, None, None, None);
    }

    #[test]
    fn port_error_exposes_code() {
        let err = PortError::new(PortErrorKind::Conflict, "cambios sin commit");
        assert_eq!(err.code(), "CONFLICT");
        assert_eq!(err.to_string(), "cambios sin commit");
    }
}
