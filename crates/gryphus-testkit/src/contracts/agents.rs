//! Contrato de `AgentRuntimePort`, usando el lenguaje de `fake-agent`
//! ([`crate::agent_script`]).
//!
//! Semántica acordada:
//! - `attach` entrega primero un `Scrollback` y después `Data` en vivo, en orden.
//! - `on_exit` se invoca exactamente una vez, también tras `kill`.
//! - Un programa inexistente falla en `spawn` con `Unavailable`.
//! - La entrada se envía como un terminal: Enter = `\r`.
//! - La salida se compara sin secuencias ANSI ni `\r` (ConPTY las añade).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use gryphus_core::ports::{
    AgentChunk, AgentExit, AgentRuntimePort, PortErrorKind, SpawnSpec, TermSize,
};
use gryphus_core::AgentId;

use super::assert_kind;
use crate::agent_script::BANNER;
use crate::sinks::CollectingChunkSink;
use crate::wait::{strip_ansi, wait_until};

pub trait AgentHarness {
    fn port(&self) -> &dyn AgentRuntimePort;
    /// Spec que lanza `fake-agent` para `agent`.
    fn spec(&self, agent: AgentId) -> SpawnSpec;
    /// Spec cuyo programa no existe.
    fn missing_program_spec(&self, agent: AgentId) -> SpawnSpec;
}

const TIMEOUT: Duration = Duration::from_secs(10);

type Exits = Arc<Mutex<Vec<AgentExit>>>;

fn id(n: u64) -> AgentId {
    AgentId::from_raw(n)
}

fn spawn<H: AgentHarness>(h: &H, agent: AgentId) -> Exits {
    let exits: Exits = Arc::default();
    let recorder = exits.clone();
    h.port()
        .spawn(
            h.spec(agent),
            Box::new(move |exit| recorder.lock().unwrap().push(exit)),
        )
        .expect("spawn");
    exits
}

fn attach<H: AgentHarness>(
    h: &H,
    agent: AgentId,
) -> (Arc<CollectingChunkSink>, gryphus_core::ports::Subscription) {
    let sink = Arc::new(CollectingChunkSink::default());
    let sub = h.port().attach(agent, sink.clone()).expect("attach");
    (sink, sub)
}

#[track_caller]
fn wait_output(sink: &CollectingChunkSink, needle: &str) {
    assert!(
        wait_until(TIMEOUT, || sink.output().contains(needle)),
        "no apareció {needle:?} en la salida:\n{}",
        sink.output()
    );
}

fn wait_exits(exits: &Exits, n: usize) -> Vec<AgentExit> {
    wait_until(TIMEOUT, || exits.lock().unwrap().len() >= n);
    exits.lock().unwrap().clone()
}

pub fn attach_receives_the_banner<H: AgentHarness>(h: &H) {
    let agent = id(1);
    spawn(h, agent);
    let (sink, _sub) = attach(h, agent);
    wait_output(&sink, BANNER);
    assert!(
        matches!(sink.chunks().first(), Some(AgentChunk::Scrollback(_))),
        "el primer mensaje debe ser Scrollback"
    );
    h.port().kill(agent).unwrap();
}

pub fn echoes_input_lines<H: AgentHarness>(h: &H) {
    let agent = id(1);
    spawn(h, agent);
    let (sink, _sub) = attach(h, agent);
    wait_output(&sink, BANNER);
    h.port().write(agent, b"hola\r").unwrap();
    wait_output(&sink, "echo: hola");
    h.port().kill(agent).unwrap();
}

pub fn exit_command_reports_the_exit_code<H: AgentHarness>(h: &H) {
    let agent = id(1);
    let exits = spawn(h, agent);
    let (sink, _sub) = attach(h, agent);
    wait_output(&sink, BANNER);
    h.port().write(agent, b"::exit 3\r").unwrap();
    assert_eq!(wait_exits(&exits, 1), [AgentExit::Code(Some(3))]);
    assert!(wait_until(TIMEOUT, || sink.exited().is_some()));
    assert_eq!(sink.exited(), Some(AgentExit::Code(Some(3))));
}

pub fn kill_reports_exit_exactly_once<H: AgentHarness>(h: &H) {
    let agent = id(1);
    let exits = spawn(h, agent);
    h.port().kill(agent).unwrap();
    assert_eq!(wait_exits(&exits, 1).len(), 1);
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(
        exits.lock().unwrap().len(),
        1,
        "on_exit se llamó más de una vez"
    );
}

pub fn late_attach_receives_scrollback<H: AgentHarness>(h: &H) {
    let agent = id(1);
    spawn(h, agent);
    let (first, _sub) = attach(h, agent);
    wait_output(&first, BANNER);
    h.port().write(agent, b"::flood 20\r").unwrap();
    wait_output(&first, "line 19");

    let (late, _late_sub) = attach(h, agent);
    match late.chunks().first() {
        Some(AgentChunk::Scrollback(bytes)) => {
            let text = strip_ansi(bytes);
            assert!(text.contains(BANNER) && text.contains("line 19"), "{text}");
        }
        other => panic!("se esperaba Scrollback primero, llegó {other:?}"),
    }
    h.port().kill(agent).unwrap();
}

pub fn flood_arrives_complete_and_in_order<H: AgentHarness>(h: &H) {
    const N: u32 = 500;
    let agent = id(1);
    spawn(h, agent);
    let (sink, _sub) = attach(h, agent);
    wait_output(&sink, BANNER);
    h.port()
        .write(agent, format!("::flood {N}\r").as_bytes())
        .unwrap();
    wait_output(&sink, &format!("line {}", N - 1));
    let numbers: Vec<u32> = sink
        .output()
        .lines()
        .filter_map(|l| l.trim().strip_prefix("line "))
        .filter_map(|n| n.trim().parse().ok())
        .collect();
    assert_eq!(numbers, (0..N).collect::<Vec<_>>());
    h.port().kill(agent).unwrap();
}

pub fn resize_live_agent<H: AgentHarness>(h: &H) {
    let agent = id(1);
    spawn(h, agent);
    h.port()
        .resize(agent, TermSize { cols: 80, rows: 24 })
        .unwrap();
    h.port().kill(agent).unwrap();
}

pub fn can_respawn_after_exit<H: AgentHarness>(h: &H) {
    let agent = id(1);
    let exits = spawn(h, agent);
    h.port().kill(agent).unwrap();
    wait_exits(&exits, 1);
    spawn(h, agent);
    let (sink, _sub) = attach(h, agent);
    wait_output(&sink, BANNER);
    h.port().kill(agent).unwrap();
}

pub fn missing_program_is_unavailable<H: AgentHarness>(h: &H) {
    let agent = id(1);
    assert_kind(
        h.port()
            .spawn(h.missing_program_spec(agent), Box::new(|_| {})),
        PortErrorKind::Unavailable,
    );
}

pub fn unknown_agent_is_not_found<H: AgentHarness>(h: &H) {
    let ghost = id(999);
    assert_kind(h.port().write(ghost, b"x"), PortErrorKind::NotFound);
    assert_kind(
        h.port().resize(ghost, TermSize::default()),
        PortErrorKind::NotFound,
    );
    assert_kind(h.port().kill(ghost), PortErrorKind::NotFound);
    assert_kind(
        h.port()
            .attach(ghost, Arc::new(CollectingChunkSink::default())),
        PortErrorKind::NotFound,
    );
}
