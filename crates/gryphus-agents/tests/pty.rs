//! Casos propios del runtime real: coalescencia, efectos en disco, shims `.cmd`.

use std::sync::Arc;
use std::time::Duration;

use gryphus_agents::PtyRuntime;
use gryphus_core::ports::{AgentChunk, AgentRuntimePort, PortErrorKind, SpawnSpec, TermSize};
use gryphus_core::{AgentId, ProviderConfig};
use gryphus_testkit::contracts::assert_kind;
use gryphus_testkit::{fake_agent_bin, wait_until, CollectingChunkSink};

const T: Duration = Duration::from_secs(10);

fn spec(agent: AgentId, program: &str, cwd: &std::path::Path) -> SpawnSpec {
    SpawnSpec::new(
        agent,
        &ProviderConfig::with_command(program),
        cwd.to_path_buf(),
        TermSize::default(),
    )
}

fn start(runtime: &PtyRuntime, cwd: &std::path::Path) -> (AgentId, Arc<CollectingChunkSink>) {
    let agent = AgentId::from_raw(1);
    let bin = fake_agent_bin();
    runtime
        .spawn(spec(agent, &bin.to_string_lossy(), cwd), Box::new(|_| {}))
        .unwrap();
    let sink = Arc::new(CollectingChunkSink::default());
    std::mem::forget(runtime.attach(agent, sink.clone()).unwrap());
    assert!(wait_until(T, || sink.output().contains("fake-agent ready")));
    (agent, sink)
}

#[test]
fn output_is_coalesced_into_few_messages() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = PtyRuntime::new();
    let (agent, sink) = start(&runtime, dir.path());
    runtime.write(agent, b"::flood 3000\r").unwrap();
    assert!(wait_until(T, || sink.output().contains("line 2999")));
    let data_messages = sink
        .chunks()
        .iter()
        .filter(|c| matches!(c, AgentChunk::Data(_)))
        .count();
    assert!(
        data_messages < 300,
        "3000 líneas llegaron en {data_messages} mensajes; se esperaba coalescencia"
    );
    runtime.kill_all();
}

#[test]
fn agent_writes_files_in_its_worktree() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = PtyRuntime::new();
    let (agent, sink) = start(&runtime, dir.path());
    runtime
        .write(agent, b"::write src/hecho.txt listo\r")
        .unwrap();
    assert!(wait_until(T, || sink
        .output()
        .contains("wrote src/hecho.txt")));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("src/hecho.txt")).unwrap(),
        "listo"
    );
    runtime.kill_all();
}

#[test]
fn writing_to_an_exited_agent_is_a_conflict() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = PtyRuntime::new();
    let (agent, sink) = start(&runtime, dir.path());
    runtime.write(agent, b"::exit 0\r").unwrap();
    assert!(wait_until(T, || sink.exited().is_some()));
    assert_kind(runtime.write(agent, b"x"), PortErrorKind::Conflict);
}

#[test]
fn scrollback_is_bounded() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = PtyRuntime::with_scrollback(4 * 1024);
    let (agent, sink) = start(&runtime, dir.path());
    runtime.write(agent, b"::flood 2000\r").unwrap();
    assert!(wait_until(T, || sink.output().contains("line 1999")));
    let late = Arc::new(CollectingChunkSink::default());
    let _sub = runtime.attach(agent, late.clone()).unwrap();
    match &late.chunks()[0] {
        AgentChunk::Scrollback(bytes) => assert!(bytes.len() <= 4 * 1024, "{}", bytes.len()),
        other => panic!("{other:?}"),
    }
    runtime.kill_all();
}

/// Los CLIs instalados con npm en Windows son `.cmd`; deben poder lanzarse.
#[cfg(windows)]
#[test]
fn npm_style_cmd_shims_are_launched() {
    let dir = tempfile::tempdir().unwrap();
    let shim_dir = tempfile::tempdir().unwrap();
    std::fs::write(
        shim_dir.path().join("agente-npm.cmd"),
        format!("@\"{}\" %*\r\n", fake_agent_bin().display()),
    )
    .unwrap();
    let runtime = PtyRuntime::new();
    let agent = AgentId::from_raw(1);
    let mut s = spec(agent, "agente-npm", dir.path());
    let path = format!(
        "{};{}",
        shim_dir.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    s.env.push(("PATH".into(), path));
    runtime.spawn(s, Box::new(|_| {})).unwrap();
    let sink = Arc::new(CollectingChunkSink::default());
    let _sub = runtime.attach(agent, sink.clone()).unwrap();
    assert!(
        wait_until(T, || sink.output().contains("fake-agent ready")),
        "{}",
        sink.output()
    );
    runtime.kill_all();
}
