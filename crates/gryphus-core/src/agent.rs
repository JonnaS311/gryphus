//! Agentes: proveedor, estado y transiciones permitidas.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::ids::{AgentId, FeatureId, WorktreeId};

/// Proveedores soportados. Conjunto cerrado; la configuración solo ajusta
/// comando, argumentos y entorno (docs/architecture/agents.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderId {
    OpenCode,
    Claude,
    Codex,
}

impl ProviderId {
    pub const ALL: [ProviderId; 3] = [ProviderId::OpenCode, ProviderId::Claude, ProviderId::Codex];

    pub fn as_str(self) -> &'static str {
        match self {
            ProviderId::OpenCode => "opencode",
            ProviderId::Claude => "claude",
            ProviderId::Codex => "codex",
        }
    }
}

impl fmt::Display for ProviderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("proveedor desconocido: {0:?} (usa opencode, claude o codex)")]
pub struct UnknownProvider(pub String);

impl FromStr for ProviderId {
    type Err = UnknownProvider;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        ProviderId::ALL
            .into_iter()
            .find(|p| p.as_str() == s)
            .ok_or_else(|| UnknownProvider(s.to_owned()))
    }
}

/// Estado del proceso de un agente.
///
/// ```text
/// Starting ──▶ Running ──▶ Exited
///     │           │
///     └───────────┴──────▶ Failed
/// Exited/Failed ──restart──▶ Starting
/// ```
/// `Starting ──▶ Exited` también es válido (el proceso puede terminar al instante).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum AgentStatus {
    Starting,
    Running,
    Exited { code: Option<i32> },
    Failed { reason: String },
}

impl AgentStatus {
    /// Activo = hay (o habrá) un proceso vivo.
    pub fn is_active(&self) -> bool {
        matches!(self, AgentStatus::Starting | AgentStatus::Running)
    }

    pub fn can_transition_to(&self, next: &AgentStatus) -> bool {
        use AgentStatus::*;
        matches!(
            (self, next),
            (Starting, Running)
                | (Starting, Exited { .. })
                | (Starting, Failed { .. })
                | (Running, Exited { .. })
                | (Running, Failed { .. })
                | (Exited { .. }, Starting)
                | (Failed { .. }, Starting)
        )
    }

    pub fn name(&self) -> &'static str {
        match self {
            AgentStatus::Starting => "starting",
            AgentStatus::Running => "running",
            AgentStatus::Exited { .. } => "exited",
            AgentStatus::Failed { .. } => "failed",
        }
    }
}

/// Un agente trabajando en un worktree de su feature. Solo [`crate::Project`] lo muta.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Agent {
    pub(crate) id: AgentId,
    pub(crate) feature: FeatureId,
    pub(crate) worktree: WorktreeId,
    pub(crate) provider: ProviderId,
    pub(crate) status: AgentStatus,
}

impl Agent {
    pub fn id(&self) -> AgentId {
        self.id
    }
    pub fn feature(&self) -> FeatureId {
        self.feature
    }
    pub fn worktree(&self) -> WorktreeId {
        self.worktree
    }
    pub fn provider(&self) -> ProviderId {
        self.provider
    }
    pub fn status(&self) -> &AgentStatus {
        &self.status
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use AgentStatus::*;

    fn all_statuses() -> [AgentStatus; 4] {
        [
            Starting,
            Running,
            Exited { code: Some(0) },
            Failed { reason: "x".into() },
        ]
    }

    #[test]
    fn allowed_transitions_match_the_documented_state_machine() {
        let allowed: Vec<(&str, &str)> = all_statuses()
            .iter()
            .flat_map(|from| {
                all_statuses()
                    .into_iter()
                    .filter(move |to| from.can_transition_to(to))
                    .map(move |to| (from.name(), to.name()))
            })
            .collect();
        assert_eq!(
            allowed,
            [
                ("starting", "running"),
                ("starting", "exited"),
                ("starting", "failed"),
                ("running", "exited"),
                ("running", "failed"),
                ("exited", "starting"),
                ("failed", "starting"),
            ]
        );
    }

    #[test]
    fn only_starting_and_running_are_active() {
        let active: Vec<_> = all_statuses()
            .iter()
            .filter(|s| s.is_active())
            .map(|s| s.name())
            .collect();
        assert_eq!(active, ["starting", "running"]);
    }

    #[test]
    fn provider_round_trips_through_str_and_serde() {
        for p in ProviderId::ALL {
            assert_eq!(p.as_str().parse::<ProviderId>(), Ok(p));
            assert_eq!(
                serde_json::to_string(&p).unwrap(),
                format!("\"{}\"", p.as_str())
            );
        }
        assert!("gpt".parse::<ProviderId>().is_err());
    }

    #[test]
    fn status_serializes_with_tag() {
        let json = serde_json::to_string(&Exited { code: Some(3) }).unwrap();
        assert_eq!(json, r#"{"state":"exited","code":3}"#);
    }
}
