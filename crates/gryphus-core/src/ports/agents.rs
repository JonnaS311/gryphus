//! Puerto del runtime de agentes. Implementación real en `gryphus-agents`
//! (PTY + supervisor + ring buffer; docs/architecture/agents.md).

use std::path::PathBuf;
use std::sync::Arc;

use crate::agent::AgentStatus;
use crate::config::ProviderConfig;
use crate::ids::AgentId;

use super::{PortResult, Subscription};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TermSize {
    pub cols: u16,
    pub rows: u16,
}

impl Default for TermSize {
    fn default() -> Self {
        Self {
            cols: 120,
            rows: 32,
        }
    }
}

/// Todo lo necesario para lanzar el proceso de un agente.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnSpec {
    pub agent: AgentId,
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    /// Directorio del worktree del agente.
    pub cwd: PathBuf,
    pub size: TermSize,
}

impl SpawnSpec {
    pub fn new(agent: AgentId, provider: &ProviderConfig, cwd: PathBuf, size: TermSize) -> Self {
        Self {
            agent,
            program: provider.command.clone(),
            args: provider.args.clone(),
            env: provider
                .env
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            cwd,
            size,
        }
    }
}

/// Fin del proceso de un agente.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentExit {
    /// Terminó; `None` si lo mató una señal.
    Code(Option<i32>),
    /// No se pudo lanzar o el runtime perdió el proceso.
    Failed(String),
}

impl From<AgentExit> for AgentStatus {
    fn from(exit: AgentExit) -> Self {
        match exit {
            AgentExit::Code(code) => AgentStatus::Exited { code },
            AgentExit::Failed(reason) => AgentStatus::Failed { reason },
        }
    }
}

/// Mensajes del flujo de salida de un agente (ver docs/architecture/ipc.md#channels).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentChunk {
    /// Una vez al suscribirse: contenido actual del ring buffer.
    Scrollback(Vec<u8>),
    /// Salida en vivo, ya coalescida.
    Data(Vec<u8>),
    Exited(AgentExit),
}

/// Receptor de la salida. Devuelve `false` si el receptor ya no existe
/// (el runtime entonces cancela la suscripción).
pub trait ChunkSink: Send + Sync {
    fn send(&self, chunk: AgentChunk) -> bool;
}

/// Se invoca una sola vez cuando el proceso termina.
pub type ExitCallback = Box<dyn FnOnce(AgentExit) + Send>;

pub trait AgentRuntimePort: Send + Sync {
    fn spawn(&self, spec: SpawnSpec, on_exit: ExitCallback) -> PortResult<()>;
    fn write(&self, agent: AgentId, bytes: &[u8]) -> PortResult<()>;
    fn resize(&self, agent: AgentId, size: TermSize) -> PortResult<()>;
    /// Termina el proceso; fuerza si no sale a tiempo. `on_exit` se invoca igualmente.
    fn kill(&self, agent: AgentId) -> PortResult<()>;
    /// Envía primero `Scrollback` y luego `Data` en vivo hasta que se suelte la suscripción.
    fn attach(&self, agent: AgentId, sink: Arc<dyn ChunkSink>) -> PortResult<Subscription>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProviderConfig;

    #[test]
    fn spawn_spec_copies_provider_settings() {
        let mut provider = ProviderConfig::with_command("claude");
        provider.args = vec!["--verbose".into()];
        provider.env.insert("K".into(), "V".into());
        let spec = SpawnSpec::new(
            AgentId::from_raw(1),
            &provider,
            PathBuf::from("wt"),
            TermSize::default(),
        );
        assert_eq!(spec.program, "claude");
        assert_eq!(spec.args, ["--verbose"]);
        assert_eq!(spec.env, [("K".to_owned(), "V".to_owned())]);
    }

    #[test]
    fn exit_maps_to_terminal_status() {
        assert_eq!(
            AgentStatus::from(AgentExit::Code(Some(0))),
            AgentStatus::Exited { code: Some(0) }
        );
        assert_eq!(
            AgentStatus::from(AgentExit::Failed("no encontrado".into())),
            AgentStatus::Failed {
                reason: "no encontrado".into()
            }
        );
    }
}
