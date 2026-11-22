//! [`FakeAgentRuntime`]: runtime de agentes en memoria que interpreta el mismo
//! lenguaje que `fake-agent` ([`crate::agent_script`]), de forma síncrona.
//!
//! Los sinks se invocan con el lock tomado (garantiza el orden Scrollback → Data);
//! por eso un sink no debe llamar de vuelta al runtime. `on_exit` se invoca sin lock.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use gryphus_core::ports::{
    AgentChunk, AgentExit, AgentRuntimePort, ChunkSink, ExitCallback, PortError, PortErrorKind,
    PortResult, SessionQuery, SpawnSpec, Subscription, TermSize,
};
use gryphus_core::{AgentId, ProviderId};

use super::{err, lock, FailSwitch};
use crate::agent_script::{self, Action};

pub const FAKE_AGENT_PROGRAM: &str = "fake-agent";
/// Igual que el valor por defecto documentado para el runtime real (2 MiB).
pub const SCROLLBACK_LIMIT: usize = 2 * 1024 * 1024;

type Sinks = Vec<(u64, Arc<dyn ChunkSink>)>;

struct Proc {
    alive: bool,
    exit: Option<AgentExit>,
    scrollback: Vec<u8>,
    sinks: Sinks,
    on_exit: Option<ExitCallback>,
    pending: String,
    last_was_cr: bool,
    written: Vec<u8>,
    size: TermSize,
}

struct Inner {
    procs: Mutex<HashMap<AgentId, Proc>>,
    programs: Mutex<HashSet<String>>,
    spawned: Mutex<Vec<SpawnSpec>>,
    /// Conversaciones "guardadas por el CLI": (proveedor, cwd, id).
    sessions: Mutex<Vec<(ProviderId, PathBuf, String)>>,
    next_sub: AtomicU64,
    fail: FailSwitch,
}

#[derive(Clone)]
pub struct FakeAgentRuntime {
    inner: Arc<Inner>,
}

impl Default for FakeAgentRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeAgentRuntime {
    /// Programas conocidos: `fake-agent` y los comandos por defecto de los tres proveedores.
    pub fn new() -> Self {
        let programs = [FAKE_AGENT_PROGRAM, "opencode", "claude", "codex"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        Self {
            inner: Arc::new(Inner {
                procs: Mutex::new(HashMap::new()),
                programs: Mutex::new(programs),
                spawned: Mutex::new(Vec::new()),
                sessions: Mutex::new(Vec::new()),
                next_sub: AtomicU64::new(0),
                fail: FailSwitch::default(),
            }),
        }
    }

    pub fn fail_next(&self, error: PortError) {
        self.inner.fail.set(error);
    }

    /// Hace que `program` se considere instalado.
    pub fn allow_program(&self, program: &str) {
        lock(&self.inner.programs).insert(program.to_owned());
    }

    pub fn spawned(&self) -> Vec<SpawnSpec> {
        lock(&self.inner.spawned).clone()
    }

    /// Simula que el CLI de `provider` guardó la conversación `id` en `cwd`.
    pub fn save_session(&self, provider: ProviderId, cwd: &Path, id: &str) {
        lock(&self.inner.sessions).push((provider, cwd.to_path_buf(), id.to_owned()));
    }

    pub fn written(&self, agent: AgentId) -> Vec<u8> {
        lock(&self.inner.procs)
            .get(&agent)
            .map(|p| p.written.clone())
            .unwrap_or_default()
    }

    pub fn is_alive(&self, agent: AgentId) -> bool {
        lock(&self.inner.procs).get(&agent).is_some_and(|p| p.alive)
    }

    pub fn size(&self, agent: AgentId) -> Option<TermSize> {
        lock(&self.inner.procs).get(&agent).map(|p| p.size)
    }

    /// Simula salida del proceso.
    pub fn emit(&self, agent: AgentId, bytes: &[u8]) {
        let mut procs = lock(&self.inner.procs);
        let Some(proc) = procs.get_mut(&agent).filter(|p| p.alive) else {
            return;
        };
        proc.scrollback.extend_from_slice(bytes);
        if proc.scrollback.len() > SCROLLBACK_LIMIT {
            let excess = proc.scrollback.len() - SCROLLBACK_LIMIT;
            proc.scrollback.drain(..excess);
        }
        proc.sinks
            .retain(|(_, sink)| sink.send(AgentChunk::Data(bytes.to_vec())));
    }

    /// Simula que el proceso termina.
    pub fn finish(&self, agent: AgentId, exit: AgentExit) {
        let on_exit = {
            let mut procs = lock(&self.inner.procs);
            let Some(proc) = procs.get_mut(&agent).filter(|p| p.alive) else {
                return;
            };
            proc.alive = false;
            proc.exit = Some(exit.clone());
            proc.sinks
                .retain(|(_, sink)| sink.send(AgentChunk::Exited(exit.clone())));
            proc.on_exit.take()
        };
        if let Some(callback) = on_exit {
            callback(exit);
        }
    }

    fn line(&self, agent: AgentId, text: &str) {
        self.emit(agent, format!("{text}\r\n").as_bytes());
    }

    fn run_script(&self, agent: AgentId, line: &str) {
        match agent_script::parse(line) {
            Action::Echo(text) => self.line(agent, &agent_script::echo_line(&text)),
            Action::Flood(n) => {
                let block: String = (0..n)
                    .map(|i| format!("{}\r\n", agent_script::flood_line(i)))
                    .collect();
                self.emit(agent, block.as_bytes());
            }
            Action::Spin(n) => {
                let mut block: String = (0..n)
                    .map(|i| format!("\r{}", agent_script::spin_frame(i)))
                    .collect();
                block.push_str(&format!("\r{}\r\n", agent_script::SPIN_DONE));
                self.emit(agent, block.as_bytes());
            }
            Action::Write { path, .. } => self.line(agent, &agent_script::wrote_line(&path)),
            Action::Unknown(text) => self.line(agent, &agent_script::unknown_line(&text)),
            Action::Exit(code) => self.finish(agent, AgentExit::Code(Some(code))),
        }
    }
}

fn not_found<T>(agent: AgentId) -> PortResult<T> {
    err(
        PortErrorKind::NotFound,
        format!("agente desconocido: {agent}"),
    )
}

impl AgentRuntimePort for FakeAgentRuntime {
    /// Como el real: con id conocido, si existe; sin él, la más antigua de `cwd` sin
    /// dueño (aquí "más antigua" = guardada antes; `since_ms` no se usa).
    fn find_session(&self, query: &SessionQuery) -> Option<String> {
        lock(&self.inner.sessions)
            .iter()
            .filter(|(p, cwd, _)| *p == query.provider && *cwd == query.cwd)
            .map(|(_, _, id)| id)
            .find(|id| match &query.known {
                Some(known) => *id == known,
                None => !query.exclude.contains(id),
            })
            .cloned()
    }

    fn spawn(&self, spec: SpawnSpec, on_exit: ExitCallback) -> PortResult<()> {
        self.inner.fail.check()?;
        if !lock(&self.inner.programs).contains(&spec.program) {
            return err(
                PortErrorKind::Unavailable,
                format!("programa no encontrado: {}", spec.program),
            );
        }
        let agent = spec.agent;
        {
            let mut procs = lock(&self.inner.procs);
            if procs.get(&agent).is_some_and(|p| p.alive) {
                return err(
                    PortErrorKind::AlreadyExists,
                    format!("{agent} ya está activo"),
                );
            }
            procs.insert(
                agent,
                Proc {
                    alive: true,
                    exit: None,
                    scrollback: Vec::new(),
                    sinks: Vec::new(),
                    on_exit: Some(on_exit),
                    pending: String::new(),
                    last_was_cr: false,
                    written: Vec::new(),
                    size: spec.size,
                },
            );
        }
        lock(&self.inner.spawned).push(spec);
        self.line(agent, agent_script::BANNER);
        Ok(())
    }

    fn write(&self, agent: AgentId, bytes: &[u8]) -> PortResult<()> {
        self.inner.fail.check()?;
        let lines = {
            let mut procs = lock(&self.inner.procs);
            let Some(proc) = procs.get_mut(&agent) else {
                return not_found(agent);
            };
            if !proc.alive {
                return err(PortErrorKind::Conflict, format!("{agent} no está activo"));
            }
            proc.written.extend_from_slice(bytes);
            // CR, LF y CRLF terminan una línea (CRLF cuenta una sola vez).
            let mut lines = Vec::new();
            for ch in String::from_utf8_lossy(bytes).chars() {
                match ch {
                    '\n' if proc.last_was_cr => proc.last_was_cr = false,
                    '\r' | '\n' => {
                        lines.push(std::mem::take(&mut proc.pending));
                        proc.last_was_cr = ch == '\r';
                    }
                    c => {
                        proc.pending.push(c);
                        proc.last_was_cr = false;
                    }
                }
            }
            lines
        };
        for line in lines {
            if !self.is_alive(agent) {
                break;
            }
            self.run_script(agent, &line);
        }
        Ok(())
    }

    fn resize(&self, agent: AgentId, size: TermSize) -> PortResult<()> {
        self.inner.fail.check()?;
        match lock(&self.inner.procs).get_mut(&agent) {
            Some(proc) => {
                proc.size = size;
                Ok(())
            }
            None => not_found(agent),
        }
    }

    fn kill(&self, agent: AgentId) -> PortResult<()> {
        self.inner.fail.check()?;
        if !lock(&self.inner.procs).contains_key(&agent) {
            return not_found(agent);
        }
        self.finish(agent, AgentExit::Code(None));
        Ok(())
    }

    fn attach(&self, agent: AgentId, sink: Arc<dyn ChunkSink>) -> PortResult<Subscription> {
        self.inner.fail.check()?;
        let id = self.inner.next_sub.fetch_add(1, Ordering::SeqCst);
        {
            let mut procs = lock(&self.inner.procs);
            let Some(proc) = procs.get_mut(&agent) else {
                return not_found(agent);
            };
            if !sink.send(AgentChunk::Scrollback(proc.scrollback.clone())) {
                return Ok(Subscription::noop());
            }
            match &proc.exit {
                Some(exit) if !proc.alive => {
                    sink.send(AgentChunk::Exited(exit.clone()));
                }
                _ => proc.sinks.push((id, sink)),
            }
        }
        let inner = self.inner.clone();
        Ok(Subscription::new(move || {
            if let Some(proc) = lock(&inner.procs).get_mut(&agent) {
                proc.sinks.retain(|(s, _)| *s != id);
            }
        }))
    }
}
