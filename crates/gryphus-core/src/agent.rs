//! Agentes: proveedor, estado y transiciones permitidas.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::ids::{AgentId, FeatureId, WorktreeId};
use crate::names::AgentName;

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

/// Cómo lanzar el CLI respecto a su conversación (ver [`ProviderId::session_args`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionLaunch<'a> {
    /// Conversación nueva, con el id que asignó Gryphus si el CLI lo admite.
    New(Option<&'a str>),
    /// Retomar la conversación con este id.
    Resume(&'a str),
}

impl ProviderId {
    /// Id para una conversación nueva cuando el CLI deja elegirlo al lanzarse
    /// (Claude: UUID; OpenCode: `ses_…`). Codex no lo permite: su id se descubre
    /// después en sus archivos de sesión. `entropy` debe ser aleatoria.
    pub fn new_session_id(self, entropy: u128, now_ms: u64) -> Option<String> {
        match self {
            ProviderId::Claude => Some(uuid_v4(entropy)),
            ProviderId::OpenCode => Some(opencode_session_id(entropy, now_ms)),
            ProviderId::Codex => None,
        }
    }

    /// Argumentos que van **antes** de los de la configuración para abrir o retomar
    /// la conversación (docs/architecture/agents.md#reanudar-conversaciones).
    pub fn session_args(self, launch: SessionLaunch<'_>) -> Vec<String> {
        let args: &[&str] = match (self, launch) {
            (ProviderId::Claude, SessionLaunch::New(Some(id))) => &["--session-id", id],
            (ProviderId::Claude, SessionLaunch::Resume(id)) => &["--resume", id],
            // `--session <id>` retoma la conversación o la crea si no existe.
            (ProviderId::OpenCode, SessionLaunch::New(Some(id)) | SessionLaunch::Resume(id)) => {
                &["--session", id]
            }
            (ProviderId::Codex, SessionLaunch::Resume(id)) => &["resume", id],
            (_, SessionLaunch::New(_)) => &[],
        };
        args.iter().map(|a| (*a).to_owned()).collect()
    }
}

/// Un id de conversación solo puede tener letras, dígitos, `-` y `_`: se pasa como
/// argumento y forma parte de nombres de archivo del CLI.
pub fn is_valid_session_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn uuid_v4(entropy: u128) -> String {
    // Versión 4 (dígito 13) y variante RFC 4122 (`10` en los bits altos del dígito 17).
    let v = (entropy & !(0xF << 76) & !(0b11 << 62)) | (0x4 << 76) | (0b10 << 62);
    let hex = format!("{v:032x}");
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

/// Mismo formato que los ids de OpenCode: `ses_` + 12 hex descendentes en el tiempo
/// (las más nuevas se listan primero) + 14 caracteres base62.
fn opencode_session_id(entropy: u128, now_ms: u64) -> String {
    const BASE62: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let time = !now_ms.wrapping_mul(0x1000) & 0xFFFF_FFFF_FFFF;
    let mut rest = entropy;
    let random: String = (0..14)
        .map(|_| {
            let c = BASE62[(rest % 62) as usize] as char;
            rest /= 62;
            c
        })
        .collect();
    format!("ses_{time:012x}{random}")
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

/// Conversación del CLI de un agente, para retomarla al relanzarlo
/// (docs/architecture/agents.md#reanudar-conversaciones).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSession {
    /// Id de la conversación en el CLI. Claude y OpenCode lo reciben de Gryphus al
    /// lanzarse; el de Codex se descubre después en sus archivos de sesión.
    pub id: Option<String>,
    /// Cuándo empezó la conversación (ms Unix): acota la búsqueda de la de Codex.
    pub started_ms: u64,
}

/// Un agente trabajando en un worktree de su feature. Solo [`crate::Project`] lo muta.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Agent {
    pub(crate) id: AgentId,
    pub(crate) feature: FeatureId,
    pub(crate) worktree: WorktreeId,
    pub(crate) provider: ProviderId,
    pub(crate) status: AgentStatus,
    /// `None` solo en proyectos guardados antes de que existieran los nombres;
    /// [`crate::Project::name_unnamed_agents`] los completa al abrir.
    #[serde(default)]
    pub(crate) name: Option<AgentName>,
    /// `None` hasta el primer lanzamiento (o en proyectos guardados antes de esto).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) session: Option<AgentSession>,
    /// Seguía activo al cerrarse el proyecto: se relanza (retomando su conversación)
    /// al volver a abrirlo.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) reopen: bool,
}

impl Agent {
    pub fn id(&self) -> AgentId {
        self.id
    }
    pub fn name(&self) -> Option<&AgentName> {
        self.name.as_ref()
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
    pub fn session(&self) -> Option<&AgentSession> {
        self.session.as_ref()
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
    fn session_ids_have_each_cli_format() {
        let uuid = ProviderId::Claude.new_session_id(u128::MAX, 0).unwrap();
        assert_eq!(uuid, "ffffffff-ffff-4fff-bfff-ffffffffffff");
        let uuid = ProviderId::Claude.new_session_id(0, 0).unwrap();
        assert_eq!(uuid, "00000000-0000-4000-8000-000000000000");

        // Casi el mismo instante que un id real de OpenCode (`ses_e06964a9affe…`,
        // creado a los 1_795_185_816_935 ms): difiere solo en los últimos dígitos.
        let ses = ProviderId::OpenCode
            .new_session_id(12345, 1_795_185_816_935)
            .unwrap();
        assert!(ses.starts_with("ses_e06964a9"), "{ses}");
        assert_eq!(ses.len(), 4 + 12 + 14);
        assert!(is_valid_session_id(&ses) && is_valid_session_id(&uuid));

        assert_eq!(ProviderId::Codex.new_session_id(1, 1), None);
        assert!(!is_valid_session_id("../x") && !is_valid_session_id(""));
    }

    #[test]
    fn session_args_open_or_resume_each_cli() {
        use SessionLaunch::*;
        let args = |p: ProviderId, l| p.session_args(l);
        assert_eq!(
            args(ProviderId::Claude, New(Some("u"))),
            ["--session-id", "u"]
        );
        assert_eq!(args(ProviderId::Claude, Resume("u")), ["--resume", "u"]);
        assert_eq!(
            args(ProviderId::OpenCode, New(Some("s"))),
            ["--session", "s"]
        );
        assert_eq!(args(ProviderId::OpenCode, Resume("s")), ["--session", "s"]);
        assert_eq!(args(ProviderId::Codex, Resume("c")), ["resume", "c"]);
        assert!(args(ProviderId::Codex, New(None)).is_empty());
        assert!(args(ProviderId::Claude, New(None)).is_empty());
    }

    #[test]
    fn status_serializes_with_tag() {
        let json = serde_json::to_string(&Exited { code: Some(3) }).unwrap();
        assert_eq!(json, r#"{"state":"exited","code":3}"#);
    }
}
