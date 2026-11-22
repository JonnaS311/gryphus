//! Eventos de dominio. Son **notificaciones, no datos**: quien los recibe vuelve a
//! pedir lo que necesita (ADR-008). `src-tauri` los reenvía a la ventana del proyecto.

use crate::agent::AgentStatus;
use crate::ids::{AgentId, FeatureId, WorktreeId};
use crate::path::RelPath;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomainEvent {
    /// Se creó o cambió una feature (worktrees, agentes).
    FeatureChanged(FeatureId),
    FeatureRemoved(FeatureId),
    AgentStatusChanged {
        agent: AgentId,
        status: AgentStatus,
    },
    /// El agente terminó lo que se le pidió y espera instrucciones (heurística de
    /// `gryphus-app`: silencio tras trabajar o aviso de terminal).
    AgentIdle {
        agent: AgentId,
    },
    FilesChanged {
        worktree: WorktreeId,
        paths: Vec<RelPath>,
    },
    /// Cambió el estado git del worktree (status, HEAD, branches).
    GitChanged {
        worktree: WorktreeId,
    },
    /// Cambió la lista de worktrees del repo.
    WorktreesChanged,
}

/// Destino de eventos. La app publica aquí; el adaptador decide cómo distribuir.
pub trait EventSink: Send + Sync {
    fn publish(&self, event: DomainEvent);
}
