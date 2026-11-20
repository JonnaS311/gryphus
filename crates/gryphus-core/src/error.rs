//! Errores del dominio. Cada variante tiene un `code` estable que viaja por IPC
//! para que la UI reaccione por código y no por texto.

use crate::agent::ProviderId;
use crate::ids::{AgentId, FeatureId, WorktreeId};
use crate::names::NameError;
use crate::path::PathError;

pub type DomainResult<T> = Result<T, DomainError>;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DomainError {
    #[error("la feature {0} no existe")]
    FeatureNotFound(FeatureId),
    #[error("el worktree {0} no existe")]
    WorktreeNotFound(WorktreeId),
    #[error("el agente {0} no existe")]
    AgentNotFound(AgentId),
    #[error("ya existe una feature llamada '{0}'")]
    DuplicateFeatureName(String),
    #[error("el worktree {worktree} ya pertenece a la feature {feature}")]
    WorktreeAlreadyAssigned {
        worktree: WorktreeId,
        feature: FeatureId,
    },
    #[error("el worktree {worktree} no pertenece a la feature {feature}")]
    WorktreeNotInFeature {
        worktree: WorktreeId,
        feature: FeatureId,
    },
    #[error("la feature {0} debe conservar al menos un worktree")]
    LastWorktree(FeatureId),
    #[error("el worktree {worktree} lo usa el agente {agent}")]
    WorktreeInUse {
        worktree: WorktreeId,
        agent: AgentId,
    },
    #[error("el worktree {0} ya no existe en disco")]
    WorktreeMissing(WorktreeId),
    #[error("el worktree {0} es el principal del repositorio")]
    MainWorktree(WorktreeId),
    #[error("la feature {0} tiene agentes activos; detenlos primero")]
    AgentsStillRunning(FeatureId),
    #[error("el agente {0} sigue activo; detenlo primero")]
    AgentStillRunning(AgentId),
    #[error("transición inválida del agente {agent}: {from} → {to}")]
    InvalidTransition {
        agent: AgentId,
        from: &'static str,
        to: &'static str,
    },
    #[error("el proveedor {0} no está configurado")]
    ProviderNotConfigured(ProviderId),
    #[error(transparent)]
    InvalidName(#[from] NameError),
    #[error(transparent)]
    InvalidPath(#[from] PathError),
    #[error("estado del proyecto inconsistente: {0}")]
    Invariant(String),
}

impl DomainError {
    /// Código estable para IPC y logs (`SCREAMING_SNAKE_CASE`).
    pub fn code(&self) -> &'static str {
        match self {
            DomainError::FeatureNotFound(_) => "FEATURE_NOT_FOUND",
            DomainError::WorktreeNotFound(_) => "WORKTREE_NOT_FOUND",
            DomainError::AgentNotFound(_) => "AGENT_NOT_FOUND",
            DomainError::DuplicateFeatureName(_) => "DUPLICATE_FEATURE_NAME",
            DomainError::WorktreeAlreadyAssigned { .. } => "WORKTREE_ALREADY_ASSIGNED",
            DomainError::WorktreeNotInFeature { .. } => "WORKTREE_NOT_IN_FEATURE",
            DomainError::LastWorktree(_) => "LAST_WORKTREE",
            DomainError::WorktreeInUse { .. } => "WORKTREE_IN_USE",
            DomainError::WorktreeMissing(_) => "WORKTREE_MISSING",
            DomainError::MainWorktree(_) => "MAIN_WORKTREE",
            DomainError::AgentsStillRunning(_) => "AGENTS_STILL_RUNNING",
            DomainError::AgentStillRunning(_) => "AGENT_STILL_RUNNING",
            DomainError::InvalidTransition { .. } => "INVALID_TRANSITION",
            DomainError::ProviderNotConfigured(_) => "PROVIDER_NOT_CONFIGURED",
            DomainError::InvalidName(_) => "INVALID_NAME",
            DomainError::InvalidPath(_) => "INVALID_PATH",
            DomainError::Invariant(_) => "INVARIANT_VIOLATION",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn codes_are_unique_and_screaming_snake_case() {
        let (f, w, a) = (
            FeatureId::from_raw(1),
            WorktreeId::from_raw(2),
            AgentId::from_raw(3),
        );
        let all = [
            DomainError::FeatureNotFound(f),
            DomainError::WorktreeNotFound(w),
            DomainError::AgentNotFound(a),
            DomainError::DuplicateFeatureName("x".into()),
            DomainError::WorktreeAlreadyAssigned {
                worktree: w,
                feature: f,
            },
            DomainError::WorktreeNotInFeature {
                worktree: w,
                feature: f,
            },
            DomainError::LastWorktree(f),
            DomainError::WorktreeInUse {
                worktree: w,
                agent: a,
            },
            DomainError::WorktreeMissing(w),
            DomainError::MainWorktree(w),
            DomainError::AgentsStillRunning(f),
            DomainError::AgentStillRunning(a),
            DomainError::InvalidTransition {
                agent: a,
                from: "x",
                to: "y",
            },
            DomainError::ProviderNotConfigured(ProviderId::Codex),
            DomainError::InvalidName(NameError::Empty),
            DomainError::InvalidPath(PathError::Absolute),
            DomainError::Invariant("x".into()),
        ];
        let codes: HashSet<_> = all.iter().map(DomainError::code).collect();
        assert_eq!(codes.len(), all.len(), "códigos duplicados");
        for code in codes {
            assert!(
                code.chars().all(|c| c.is_ascii_uppercase() || c == '_'),
                "{code}"
            );
        }
    }

    #[test]
    fn messages_are_human_readable() {
        let err = DomainError::LastWorktree(FeatureId::from_raw(4));
        assert_eq!(
            err.to_string(),
            "la feature feat-4 debe conservar al menos un worktree"
        );
    }
}
