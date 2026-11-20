//! Adaptador `AgentRuntimePort` sobre pseudo-terminales (`portable-pty`).
//!
//! Por agente hay tres hilos:
//! - **lector**: lee la PTY y acumula bytes pendientes;
//! - **flusher**: envía de inmediato tras un reposo y, si la salida no para, como mucho una vez por [`COALESCE`]; añade al
//!   ring buffer y los envía a los suscriptores, todo bajo el mismo lock, así que
//!   un `attach` nunca ve Data duplicada ni desordenada respecto al Scrollback;
//! - **waiter**: espera al proceso, deja drenar la salida, cierra la PTY y notifica
//!   la salida exactamente una vez.
//!
//! Ver docs/architecture/agents.md.
#![forbid(unsafe_code)]

mod resolve;

use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use gryphus_core::ports::{
    AgentChunk, AgentExit, AgentRuntimePort, ChunkSink, ExitCallback, PortError, PortErrorKind,
    PortResult, SpawnSpec, Subscription, TermSize,
};
use gryphus_core::AgentId;
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize};

pub use resolve::resolve_program;

/// Ventana máxima de agrupación de salida (~1 frame).
pub const COALESCE: Duration = Duration::from_millis(16);
/// Scrollback por agente.
pub const DEFAULT_SCROLLBACK: usize = 2 * 1024 * 1024;
/// Tras salir el proceso, se espera a que la salida quede inactiva este tiempo…
const DRAIN_IDLE: Duration = Duration::from_millis(100);
/// …con este máximo, antes de cerrar la PTY.
const DRAIN_MAX: Duration = Duration::from_secs(2);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

fn io_error(e: impl std::fmt::Display) -> PortError {
    PortError::new(PortErrorKind::Io, e.to_string())
}

#[derive(Clone)]
pub struct PtyRuntime {
    inner: Arc<Inner>,
}

struct Inner {
    agents: Mutex<HashMap<AgentId, Arc<AgentProc>>>,
    scrollback_limit: usize,
    next_sub: AtomicU64,
}

/// Estado de salida protegido por un único lock (orden Scrollback → Data).
struct Output {
    scrollback: VecDeque<u8>,
    sinks: Vec<(u64, Arc<dyn ChunkSink>)>,
    exit: Option<AgentExit>,
}

struct Pending {
    bytes: Vec<u8>,
    reader_done: bool,
}

struct AgentProc {
    alive: AtomicBool,
    killed: AtomicBool,
    writer: Mutex<Option<Box<dyn Write + Send>>>,
    master: Mutex<Option<Box<dyn MasterPty + Send>>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    output: Mutex<Output>,
    pending: Mutex<Pending>,
    pending_cv: Condvar,
    last_read: Mutex<Instant>,
}

impl Default for PtyRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl PtyRuntime {
    pub fn new() -> Self {
        Self::with_scrollback(DEFAULT_SCROLLBACK)
    }

    pub fn with_scrollback(limit: usize) -> Self {
        Self {
            inner: Arc::new(Inner {
                agents: Mutex::new(HashMap::new()),
                scrollback_limit: limit,
                next_sub: AtomicU64::new(0),
            }),
        }
    }

    fn get(&self, agent: AgentId) -> PortResult<Arc<AgentProc>> {
        lock(&self.inner.agents)
            .get(&agent)
            .cloned()
            .ok_or_else(|| {
                PortError::new(
                    PortErrorKind::NotFound,
                    format!("agente desconocido: {agent}"),
                )
            })
    }

    /// Termina todos los agentes activos (cierre de la app).
    pub fn kill_all(&self) {
        let agents: Vec<_> = lock(&self.inner.agents).keys().copied().collect();
        for agent in agents {
            let _ = self.kill(agent);
        }
    }
}

impl AgentProc {
    /// Añade un lote al ring buffer y lo envía a los suscriptores.
    fn flush(&self, bytes: Vec<u8>, limit: usize) {
        let mut out = lock(&self.output);
        out.scrollback.extend(bytes.iter().copied());
        let excess = out.scrollback.len().saturating_sub(limit);
        out.scrollback.drain(..excess);
        out.sinks
            .retain(|(_, sink)| sink.send(AgentChunk::Data(bytes.clone())));
    }
}

/// Pregunta de posición del cursor (DSR) con la que ConPTY abre la sesión.
const DSR_QUERY: &[u8] = b"\x1b[6n";
/// Respuesta que desbloquea a ConPTY.
const DSR_REPLY: &[u8] = b"\x1b[1;1R";
/// Solo se busca el DSR inicial en los primeros bytes de la sesión.
const HANDSHAKE_WINDOW: usize = 1024;

/// ConPTY no produce salida hasta que alguien responde a su DSR inicial. Un agente
/// puede arrancar sin ninguna vista conectada, así que lo responde el runtime y lo
/// quita de la salida (si xterm.js también respondiera, el agente recibiría
/// basura en su entrada).
fn answer_conpty_handshake(proc: &AgentProc, chunk: &[u8]) -> Option<Vec<u8>> {
    let pos = chunk
        .windows(DSR_QUERY.len())
        .position(|w| w == DSR_QUERY)?;
    if let Some(writer) = lock(&proc.writer).as_mut() {
        let _ = writer.write_all(DSR_REPLY).and_then(|_| writer.flush());
    }
    Some([&chunk[..pos], &chunk[pos + DSR_QUERY.len()..]].concat())
}

fn reader_loop(proc: Arc<AgentProc>, mut reader: Box<dyn Read + Send>) {
    let mut buf = vec![0u8; 16 * 1024];
    let mut handshake_pending = cfg!(windows);
    let mut seen = 0usize;
    loop {
        match reader.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                *lock(&proc.last_read) = Instant::now();
                let mut chunk = &buf[..n];
                let rewritten;
                if handshake_pending {
                    if let Some(rest) = answer_conpty_handshake(&proc, chunk) {
                        rewritten = rest;
                        chunk = &rewritten;
                        handshake_pending = false;
                    }
                    seen += n;
                    handshake_pending &= seen < HANDSHAKE_WINDOW;
                }
                if chunk.is_empty() {
                    continue;
                }
                let mut p = lock(&proc.pending);
                p.bytes.extend_from_slice(chunk);
                proc.pending_cv.notify_one();
            }
        }
    }
    mark_reader_done(&proc);
}

fn mark_reader_done(proc: &AgentProc) {
    lock(&proc.pending).reader_done = true;
    proc.pending_cv.notify_one();
}

/// Throttle con flanco de subida: si no se envió nada en los últimos
/// [`COALESCE`], el lote sale **de inmediato** (eco de teclas sin espera); si la
/// salida no para, se agrupa y sale como mucho una vez por ventana.
fn flusher_loop(proc: Arc<AgentProc>, limit: usize) {
    let mut last_flush: Option<Instant> = None;
    loop {
        let batch = {
            let mut p = lock(&proc.pending);
            while p.bytes.is_empty() && !p.reader_done {
                p = proc.pending_cv.wait(p).unwrap_or_else(|e| e.into_inner());
            }
            if p.bytes.is_empty() && p.reader_done {
                return;
            }
            let wait = last_flush
                .map(|t| t + COALESCE)
                .and_then(|ready_at| ready_at.checked_duration_since(Instant::now()));
            if let Some(left) = wait.filter(|_| !p.reader_done) {
                // `thread::sleep` usa temporizadores de alta resolución en Windows;
                // `Condvar::wait_timeout` redondearía a ~15,6 ms (medido: +30 ms de eco).
                // Mientras dormimos, el lector sigue acumulando: eso es la coalescencia.
                drop(p);
                thread::sleep(left);
                p = lock(&proc.pending);
            }
            std::mem::take(&mut p.bytes)
        };
        proc.flush(batch, limit);
        last_flush = Some(Instant::now());
    }
}

impl AgentRuntimePort for PtyRuntime {
    fn spawn(&self, spec: SpawnSpec, on_exit: ExitCallback) -> PortResult<()> {
        if lock(&self.inner.agents)
            .get(&spec.agent)
            .is_some_and(|p| p.alive.load(Ordering::SeqCst))
        {
            return Err(PortError::new(
                PortErrorKind::AlreadyExists,
                format!("{} ya está activo", spec.agent),
            ));
        }
        let path_override = spec
            .env
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("PATH"))
            .map(|(_, v)| v.clone());
        let (program, prefix_args) =
            resolve::resolve_program(&spec.program, path_override.as_deref()).ok_or_else(|| {
                PortError::new(
                    PortErrorKind::Unavailable,
                    format!("no se encontró el programa {:?}", spec.program),
                )
            })?;

        let pair = native_pty_system()
            .openpty(pty_size(spec.size))
            .map_err(io_error)?;
        let mut cmd = CommandBuilder::new(&program);
        cmd.args(&prefix_args);
        cmd.args(&spec.args);
        cmd.cwd(&spec.cwd);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        for (k, v) in &spec.env {
            cmd.env(k, v);
        }
        let mut child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| PortError::new(PortErrorKind::Unavailable, e.to_string()))?;
        drop(pair.slave);
        let reader = pair.master.try_clone_reader().map_err(io_error)?;
        let writer = pair.master.take_writer().map_err(io_error)?;

        let proc = Arc::new(AgentProc {
            alive: AtomicBool::new(true),
            killed: AtomicBool::new(false),
            writer: Mutex::new(Some(writer)),
            master: Mutex::new(Some(pair.master)),
            killer: Mutex::new(child.clone_killer()),
            output: Mutex::new(Output {
                scrollback: VecDeque::new(),
                sinks: Vec::new(),
                exit: None,
            }),
            pending: Mutex::new(Pending {
                bytes: Vec::new(),
                reader_done: false,
            }),
            pending_cv: Condvar::new(),
            last_read: Mutex::new(Instant::now()),
        });
        lock(&self.inner.agents).insert(spec.agent, proc.clone());

        let limit = self.inner.scrollback_limit;
        let name = |what: &str| format!("gryphus-agent-{}-{what}", spec.agent.get());
        let r = proc.clone();
        thread::Builder::new()
            .name(name("reader"))
            .spawn(move || reader_loop(r, reader))
            .map_err(io_error)?;
        let f = proc.clone();
        let flusher = thread::Builder::new()
            .name(name("flusher"))
            .spawn(move || flusher_loop(f, limit))
            .map_err(io_error)?;
        thread::Builder::new()
            .name(name("waiter"))
            .spawn(move || {
                let status = child.wait();
                // Deja drenar la salida pendiente antes de cerrar la PTY
                // (ConPTY no entrega EOF hasta que se cierra).
                let started = Instant::now();
                while started.elapsed() < DRAIN_MAX && lock(&proc.last_read).elapsed() < DRAIN_IDLE
                {
                    thread::sleep(Duration::from_millis(20));
                }
                lock(&proc.writer).take();
                lock(&proc.master).take();
                // Cerrar la PTY debería dar EOF al lector; si no llega a tiempo,
                // se da por terminado para no bloquear la notificación de salida.
                let closed_at = Instant::now();
                while !lock(&proc.pending).reader_done && closed_at.elapsed() < DRAIN_MAX {
                    thread::sleep(Duration::from_millis(20));
                }
                mark_reader_done(&proc);
                let _ = flusher.join();

                let exit = if proc.killed.load(Ordering::SeqCst) {
                    AgentExit::Code(None)
                } else {
                    match status {
                        Ok(s) => AgentExit::Code(Some(s.exit_code() as i32)),
                        Err(e) => AgentExit::Failed(e.to_string()),
                    }
                };
                proc.alive.store(false, Ordering::SeqCst);
                {
                    let mut out = lock(&proc.output);
                    out.exit = Some(exit.clone());
                    out.sinks
                        .retain(|(_, sink)| sink.send(AgentChunk::Exited(exit.clone())));
                    out.sinks.clear();
                }
                on_exit(exit);
            })
            .map_err(io_error)?;
        Ok(())
    }

    fn write(&self, agent: AgentId, bytes: &[u8]) -> PortResult<()> {
        let proc = self.get(agent)?;
        let mut writer = lock(&proc.writer);
        let Some(w) = writer
            .as_mut()
            .filter(|_| proc.alive.load(Ordering::SeqCst))
        else {
            return Err(PortError::new(
                PortErrorKind::Conflict,
                format!("{agent} no está activo"),
            ));
        };
        w.write_all(bytes).and_then(|_| w.flush()).map_err(io_error)
    }

    fn resize(&self, agent: AgentId, size: TermSize) -> PortResult<()> {
        let proc = self.get(agent)?;
        let master = lock(&proc.master);
        match master.as_ref() {
            Some(m) => m.resize(pty_size(size)).map_err(io_error),
            None => Ok(()), // ya terminó: no hay nada que redimensionar
        }
    }

    fn kill(&self, agent: AgentId) -> PortResult<()> {
        let proc = self.get(agent)?;
        if proc.alive.load(Ordering::SeqCst) {
            proc.killed.store(true, Ordering::SeqCst);
            // Puede fallar si el proceso acaba de terminar por su cuenta; no es un error.
            let _ = lock(&proc.killer).kill();
        }
        Ok(())
    }

    fn attach(&self, agent: AgentId, sink: Arc<dyn ChunkSink>) -> PortResult<Subscription> {
        let proc = self.get(agent)?;
        let id = self.inner.next_sub.fetch_add(1, Ordering::SeqCst);
        {
            let mut out = lock(&proc.output);
            let scrollback: Vec<u8> = out.scrollback.iter().copied().collect();
            if !sink.send(AgentChunk::Scrollback(scrollback)) {
                return Ok(Subscription::noop());
            }
            if let Some(exit) = out.exit.clone() {
                sink.send(AgentChunk::Exited(exit));
                return Ok(Subscription::noop());
            }
            out.sinks.push((id, sink));
        }
        Ok(Subscription::new(move || {
            lock(&proc.output).sinks.retain(|(s, _)| *s != id);
        }))
    }
}

fn pty_size(size: TermSize) -> PtySize {
    PtySize {
        rows: size.rows,
        cols: size.cols,
        pixel_width: 0,
        pixel_height: 0,
    }
}
