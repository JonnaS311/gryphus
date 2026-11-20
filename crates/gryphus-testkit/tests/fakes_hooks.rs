//! Ganchos de los fakes que no cubre el contrato (los usa `gryphus-app`).

use std::path::PathBuf;
use std::sync::Arc;

use gryphus_core::ports::{
    AgentExit, AgentRuntimePort, FsPort, GitPort, PortError, PortErrorKind, SpawnSpec, StorePort,
    TermSize,
};
use gryphus_core::{AgentId, DomainEvent, EventSink, FeatureId, Project, ProviderConfig, RelPath};
use gryphus_testkit::{
    CollectingChunkSink, CollectingFsSink, FakeAgentRuntime, FakeFs, FakeGit, FakeStore,
    RecordingEventSink,
};

fn conflict() -> PortError {
    PortError::new(PortErrorKind::Conflict, "simulado")
}

#[test]
fn fail_next_fails_exactly_one_call() {
    let git = FakeGit::new("/r");
    git.fail_next(conflict());
    assert_eq!(git.list_worktrees(&PathBuf::from("/r")), Err(conflict()));
    assert!(git.list_worktrees(&PathBuf::from("/r")).is_ok());

    let fs = FakeFs::new("/r");
    fs.fail_next(conflict());
    assert!(fs.list_dir(&PathBuf::from("/r"), &RelPath::root()).is_err());

    let store = FakeStore::default();
    store.fail_next(conflict());
    assert!(store.load_config().is_err());
    assert!(store.load_config().is_ok());

    let runtime = FakeAgentRuntime::new();
    runtime.fail_next(conflict());
    assert!(runtime.kill(AgentId::from_raw(1)).is_err());
}

#[test]
fn agent_runtime_hooks_simulate_output_and_exit() {
    let runtime = FakeAgentRuntime::new();
    let agent = AgentId::from_raw(7);
    let spec = SpawnSpec::new(
        agent,
        &ProviderConfig::with_command("claude"),
        PathBuf::from("/wt"),
        TermSize::default(),
    );
    let exits = Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorder = exits.clone();
    runtime
        .spawn(
            spec.clone(),
            Box::new(move |e| recorder.lock().unwrap().push(e)),
        )
        .unwrap();
    assert_eq!(runtime.spawned(), [spec]);

    let sink = Arc::new(CollectingChunkSink::default());
    let _sub = runtime.attach(agent, sink.clone()).unwrap();
    runtime.emit(agent, b"salida simulada\r\n");
    runtime.write(agent, b"hola\r\n").unwrap();
    runtime
        .resize(agent, TermSize { cols: 10, rows: 5 })
        .unwrap();
    assert!(sink.output().contains("salida simulada"));
    assert!(sink.output().contains("echo: hola"));
    assert_eq!(runtime.written(agent), b"hola\r\n");
    assert_eq!(runtime.size(agent), Some(TermSize { cols: 10, rows: 5 }));

    runtime.finish(agent, AgentExit::Failed("boom".into()));
    assert!(!runtime.is_alive(agent));
    assert_eq!(*exits.lock().unwrap(), [AgentExit::Failed("boom".into())]);
    assert_eq!(sink.exited(), Some(AgentExit::Failed("boom".into())));
}

#[test]
fn crlf_counts_as_a_single_line_break() {
    let runtime = FakeAgentRuntime::new();
    let agent = AgentId::from_raw(1);
    runtime
        .spawn(
            SpawnSpec::new(
                agent,
                &ProviderConfig::with_command("codex"),
                PathBuf::from("/wt"),
                TermSize::default(),
            ),
            Box::new(|_| {}),
        )
        .unwrap();
    let sink = Arc::new(CollectingChunkSink::default());
    let _sub = runtime.attach(agent, sink.clone()).unwrap();
    runtime.write(agent, b"a\r\nb\nc\r").unwrap();
    let echoes: Vec<_> = sink
        .output()
        .lines()
        .filter(|l| l.starts_with("echo:"))
        .map(str::to_owned)
        .collect();
    assert_eq!(echoes, ["echo: a", "echo: b", "echo: c"]);
}

#[test]
fn closed_chunk_sink_is_dropped_by_the_runtime() {
    let runtime = FakeAgentRuntime::new();
    let agent = AgentId::from_raw(1);
    runtime
        .spawn(
            SpawnSpec::new(
                agent,
                &ProviderConfig::with_command("claude"),
                PathBuf::from("/wt"),
                TermSize::default(),
            ),
            Box::new(|_| {}),
        )
        .unwrap();
    let sink = Arc::new(CollectingChunkSink::default());
    let _sub = runtime.attach(agent, sink.clone()).unwrap();
    sink.close();
    runtime.emit(agent, b"x");
    assert_eq!(sink.chunks().len(), 1, "solo el Scrollback inicial");
}

#[test]
fn fs_write_through_port_notifies_watchers() {
    let fs = FakeFs::new("/r");
    let sink = Arc::new(CollectingFsSink::default());
    let _sub = fs.watch(&PathBuf::from("/r"), sink.clone()).unwrap();
    fs.write_file(
        &PathBuf::from("/r"),
        &RelPath::parse("a.txt").unwrap(),
        b"x",
    )
    .unwrap();
    assert!(sink.contains("a.txt"));
}

#[test]
fn store_counts_saves() {
    let store = FakeStore::default();
    store
        .save_project(&Project::new(PathBuf::from("/r")))
        .unwrap();
    assert_eq!(store.save_count(), 1);
}

#[test]
fn recording_sink_keeps_and_takes_events() {
    let sink = RecordingEventSink::default();
    sink.publish(DomainEvent::FeatureChanged(FeatureId::from_raw(1)));
    assert_eq!(sink.events().len(), 1);
    assert_eq!(sink.take().len(), 1);
    assert!(sink.events().is_empty());
}
