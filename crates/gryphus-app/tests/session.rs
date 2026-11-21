//! Casos de uso de `ProjectSession` con fakes de todos los puertos.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gryphus_app::{AppError, Ports, ProjectSession, WorktreeChoice};
use gryphus_core::ports::{
    AgentChunk, AgentExit, DiffBase, FileContent, FileState, GitPort, NewWorktree, PortError,
    PortErrorKind, StorePort,
};
use gryphus_core::{
    AgentStatus, BranchName, Config, DomainEvent, FeatureId, ProjectId, ProviderConfig, ProviderId,
    WorktreeId,
};
use gryphus_testkit::{
    CollectingChunkSink, FakeAgentRuntime, FakeFs, FakeGit, FakeStore, RecordingEventSink,
};

const ROOT: &str = "/repo";

struct World {
    git: Arc<FakeGit>,
    agents: FakeAgentRuntime,
    fs: FakeFs,
    store: Arc<FakeStore>,
    events: Arc<RecordingEventSink>,
}

impl World {
    fn new() -> Self {
        Self::with_config(Config::default())
    }

    fn with_config(config: Config) -> Self {
        let git = FakeGit::new(ROOT);
        git.write_file(Path::new(ROOT), "README.md", "hola\n");
        git.commit_all(Path::new(ROOT), "init");
        Self {
            git: Arc::new(git),
            agents: FakeAgentRuntime::new(),
            fs: FakeFs::new(ROOT),
            store: Arc::new(FakeStore::with_config(config)),
            events: Arc::new(RecordingEventSink::default()),
        }
    }

    fn ports(&self) -> Ports {
        Ports {
            git: self.git.clone(),
            agents: Arc::new(self.agents.clone()),
            fs: Arc::new(self.fs.clone()),
            store: self.store.clone(),
            events: self.events.clone(),
        }
    }

    fn open(&self) -> ProjectSession {
        ProjectSession::open(Path::new(ROOT), self.ports()).unwrap()
    }

    fn saved(&self) -> gryphus_core::Project {
        self.store
            .load_project(&ProjectId::from_canonical_root(Path::new(ROOT)))
            .unwrap()
            .expect("proyecto guardado")
    }
}

fn main_wt(s: &ProjectSession) -> WorktreeId {
    s.project()
        .worktrees()
        .iter()
        .find(|w| w.is_main())
        .unwrap()
        .id()
}

fn new_feature(s: &ProjectSession, name: &str) -> (FeatureId, WorktreeId) {
    let f = s
        .create_feature(name, WorktreeChoice::New { base: None })
        .unwrap();
    let wt = s.project().feature(f).unwrap().worktrees()[0];
    (f, wt)
}

fn code<T: std::fmt::Debug>(r: Result<T, AppError>) -> &'static str {
    r.expect_err("se esperaba error").code()
}

// ── Apertura ─────────────────────────────────────────────────────────────────

#[test]
fn opening_registers_the_main_worktree_and_saves() {
    let w = World::new();
    let s = w.open();
    assert_eq!(s.root(), Path::new(ROOT));
    assert_eq!(s.project().worktrees().len(), 1);
    assert!(s.project().worktrees()[0].is_main());
    assert_eq!(w.saved(), s.project());
}

#[test]
fn reopening_restores_features_and_marks_agents_exited() {
    let w = World::new();
    let s = w.open();
    let (f, wt) = new_feature(&s, "login");
    let agent = s.spawn_agent(f, wt, ProviderId::Claude).unwrap();
    drop(s);

    let reopened = w.open();
    let p = reopened.project();
    assert_eq!(p.features()[0].name().as_str(), "login");
    assert_eq!(
        p.agent(agent).unwrap().status(),
        &AgentStatus::Exited { code: None }
    );
}

#[test]
fn opening_outside_a_repo_fails() {
    let w = World::new();
    let r = ProjectSession::open(Path::new("/otro"), w.ports());
    assert_eq!(code(r), "NOT_FOUND");
}

// ── Features ─────────────────────────────────────────────────────────────────

#[test]
fn new_feature_creates_its_branch_and_worktree_next_to_the_repo() {
    let w = World::new();
    let s = w.open();
    let (f, wt) = new_feature(&s, "login");

    let p = s.project();
    let worktree = p.worktree(wt).unwrap();
    assert_eq!(worktree.branch().unwrap().as_str(), "feat/login");
    assert_eq!(
        worktree.path(),
        s.config().worktree_path(
            Path::new(ROOT),
            &gryphus_core::FeatureName::parse("login").unwrap()
        )
    );
    assert_eq!(w.git.list_worktrees(Path::new(ROOT)).unwrap().len(), 2);
    assert!(w.events.events().contains(&DomainEvent::FeatureChanged(f)));
    assert!(w.events.events().contains(&DomainEvent::WorktreesChanged));
    assert_eq!(w.saved(), p);
}

#[test]
fn duplicate_feature_name_does_not_touch_git() {
    let w = World::new();
    let s = w.open();
    new_feature(&s, "login");
    let r = s.create_feature("LOGIN", WorktreeChoice::New { base: None });
    assert_eq!(code(r), "DUPLICATE_FEATURE_NAME");
    assert_eq!(w.git.list_worktrees(Path::new(ROOT)).unwrap().len(), 2);
}

#[test]
fn invalid_feature_name_is_rejected() {
    let w = World::new();
    let s = w.open();
    let r = s.create_feature("con espacio", WorktreeChoice::New { base: None });
    assert_eq!(code(r), "INVALID_NAME");
}

#[test]
fn git_failure_leaves_no_feature() {
    let w = World::new();
    let s = w.open();
    w.git
        .fail_next(PortError::new(PortErrorKind::Conflict, "lock"));
    let r = s.create_feature("login", WorktreeChoice::New { base: None });
    assert_eq!(code(r), "CONFLICT");
    assert!(s.project().features().is_empty());
}

#[test]
fn domain_failure_after_creating_the_worktree_rolls_it_back() {
    let w = World::new();
    let s = w.open();
    // El worktree principal ya respalda otra feature con otro nombre: el caso de
    // uso crea el worktree nuevo, pero un fallo de persistencia lo hace fallar.
    let _ = main_wt(&s);
    w.store
        .fail_next(PortError::new(PortErrorKind::Io, "disco lleno"));
    let r = s.create_feature("login", WorktreeChoice::New { base: None });
    assert_eq!(code(r), "IO");
    assert_eq!(
        w.git.list_worktrees(Path::new(ROOT)).unwrap().len(),
        1,
        "el worktree creado se eliminó"
    );
    assert_eq!(
        s.project().worktrees().len(),
        1,
        "y se olvidó en el registro"
    );
}

#[test]
fn feature_can_use_an_existing_worktree() {
    let w = World::new();
    let s = w.open();
    w.git
        .add_worktree(
            Path::new(ROOT),
            &NewWorktree {
                path: PathBuf::from("/externo"),
                branch: BranchName::parse("externo").unwrap(),
                create_branch: true,
                base: None,
            },
        )
        .unwrap();
    assert!(s.refresh_worktrees().unwrap());
    let wt = s
        .project()
        .worktree_by_path(Path::new("/externo"))
        .unwrap()
        .id();
    let f = s
        .create_feature("externo", WorktreeChoice::Existing(wt))
        .unwrap();
    assert_eq!(s.project().feature(f).unwrap().worktrees(), [wt]);
}

#[test]
fn delete_feature_requires_stopped_agents_and_removes_its_worktree() {
    let w = World::new();
    let s = w.open();
    let (f, wt) = new_feature(&s, "login");
    let agent = s.spawn_agent(f, wt, ProviderId::Codex).unwrap();

    assert_eq!(
        code(s.delete_feature(f, true, false)),
        "AGENTS_STILL_RUNNING"
    );
    assert_eq!(
        w.git.list_worktrees(Path::new(ROOT)).unwrap().len(),
        2,
        "git intacto"
    );

    s.stop_agent(agent).unwrap();
    s.delete_feature(f, true, false).unwrap();
    assert!(s.project().features().is_empty());
    assert_eq!(w.git.list_worktrees(Path::new(ROOT)).unwrap().len(), 1);
    assert_eq!(
        s.project().worktrees().len(),
        1,
        "el registro olvida el worktree"
    );
    assert!(w.events.events().contains(&DomainEvent::FeatureRemoved(f)));
}

#[test]
fn deleting_a_dirty_worktree_requires_force() {
    let w = World::new();
    let s = w.open();
    let (f, wt) = new_feature(&s, "login");
    let path = s.project().worktree(wt).unwrap().path().to_path_buf();
    w.git.write_file(&path, "sucio.txt", "x");

    assert_eq!(code(s.delete_feature(f, true, false)), "CONFLICT");
    assert_eq!(s.project().features().len(), 1, "la feature sigue");
    s.delete_feature(f, true, true).unwrap();
    assert!(s.project().features().is_empty());
}

/// Con varios worktrees, uno limpio y otro sucio: sin `force` no se debe borrar
/// ninguno (antes se borraba el limpio y la feature quedaba apuntando a un
/// worktree inexistente).
#[test]
fn dirty_worktree_aborts_deletion_before_touching_any_other() {
    let w = World::new();
    let s = w.open();
    let (f, wt) = new_feature(&s, "login");
    w.git
        .add_worktree(
            Path::new(ROOT),
            &NewWorktree {
                path: PathBuf::from("/externo"),
                branch: BranchName::parse("externo").unwrap(),
                create_branch: true,
                base: None,
            },
        )
        .unwrap();
    s.refresh_worktrees().unwrap();
    let extra = s
        .project()
        .worktree_by_path(Path::new("/externo"))
        .unwrap()
        .id();
    s.attach_worktree(f, extra).unwrap();
    // El sucio va segundo: el limpio (wt) se procesaría primero.
    w.git.write_file(Path::new("/externo"), "sucio.txt", "x");

    assert_eq!(code(s.delete_feature(f, true, false)), "CONFLICT");
    assert_eq!(
        w.git.list_worktrees(Path::new(ROOT)).unwrap().len(),
        3,
        "ningún worktree se borró"
    );
    let p = s.project();
    assert!(!p.worktree(wt).unwrap().is_missing());
    assert_eq!(p.feature(f).unwrap().worktrees(), [wt, extra]);
}

#[test]
fn restarting_an_agent_publishes_the_starting_state() {
    let w = World::new();
    let s = w.open();
    let (f, wt) = new_feature(&s, "login");
    let agent = s.spawn_agent(f, wt, ProviderId::Claude).unwrap();
    s.stop_agent(agent).unwrap();
    s.restart_agent(agent).unwrap();
    assert!(w
        .events
        .events()
        .contains(&DomainEvent::AgentStatusChanged {
            agent,
            status: AgentStatus::Starting
        }));
}

/// Los guardados concurrentes deben persistir siempre el estado más reciente.
#[test]
fn concurrent_mutations_persist_the_latest_state() {
    let w = World::new();
    let s = w.open();
    let threads: Vec<_> = (0..4)
        .map(|i| {
            let s = s.clone();
            std::thread::spawn(move || {
                for j in 0..10 {
                    s.create_feature(&format!("f{i}-{j}"), WorktreeChoice::New { base: None })
                        .unwrap();
                }
            })
        })
        .collect();
    for t in threads {
        t.join().unwrap();
    }
    assert_eq!(s.project().features().len(), 40);
    assert_eq!(w.saved(), s.project());
}

#[test]
fn delete_feature_can_keep_its_worktree() {
    let w = World::new();
    let s = w.open();
    let (f, _) = new_feature(&s, "login");
    s.delete_feature(f, false, false).unwrap();
    assert_eq!(w.git.list_worktrees(Path::new(ROOT)).unwrap().len(), 2);
    assert_eq!(s.project().unassigned_worktrees().count(), 2);
}

#[test]
fn attach_and_detach_worktrees() {
    let w = World::new();
    let s = w.open();
    let (f, wt) = new_feature(&s, "login");
    let main = main_wt(&s);
    s.attach_worktree(f, main).unwrap();
    assert_eq!(s.project().feature(f).unwrap().worktrees(), [wt, main]);
    s.detach_worktree(f, main).unwrap();
    assert_eq!(code(s.detach_worktree(f, wt)), "LAST_WORKTREE");
}

// ── Agentes ──────────────────────────────────────────────────────────────────

#[test]
fn spawn_agent_launches_the_configured_command_in_its_worktree() {
    let mut config = Config::default();
    let mut claude = ProviderConfig::with_command("claude");
    claude.args = vec!["--modelo".into(), "x".into()];
    config.agents.insert(ProviderId::Claude, claude);
    let w = World::with_config(config);
    let s = w.open();
    let (f, wt) = new_feature(&s, "login");

    let agent = s.spawn_agent(f, wt, ProviderId::Claude).unwrap();
    let spec = &w.agents.spawned()[0];
    assert_eq!(spec.program, "claude");
    assert_eq!(spec.args, ["--modelo", "x"]);
    assert_eq!(spec.cwd, s.project().worktree(wt).unwrap().path());
    assert_eq!(
        s.project().agent(agent).unwrap().status(),
        &AgentStatus::Running
    );
    assert!(w
        .events
        .events()
        .contains(&DomainEvent::AgentStatusChanged {
            agent,
            status: AgentStatus::Running
        }));
}

#[test]
fn agent_exit_is_recorded_saved_and_published() {
    let w = World::new();
    let s = w.open();
    let (f, wt) = new_feature(&s, "login");
    let agent = s.spawn_agent(f, wt, ProviderId::Claude).unwrap();
    w.agents.finish(agent, AgentExit::Code(Some(2)));

    let exited = AgentStatus::Exited { code: Some(2) };
    assert_eq!(s.project().agent(agent).unwrap().status(), &exited);
    assert_eq!(w.saved().agent(agent).unwrap().status(), &exited);
    assert!(w
        .events
        .events()
        .contains(&DomainEvent::AgentStatusChanged {
            agent,
            status: exited
        }));
}

#[test]
fn failed_launch_marks_the_agent_failed() {
    let mut config = Config::default();
    config.agents.insert(
        ProviderId::Codex,
        ProviderConfig::with_command("no-instalado"),
    );
    let w = World::with_config(config);
    let s = w.open();
    let (f, wt) = new_feature(&s, "login");
    let r = s.spawn_agent(f, wt, ProviderId::Codex);
    assert_eq!(code(r), "UNAVAILABLE");
    let agent = s.project().agents()[0].id();
    assert!(matches!(
        s.project().agent(agent).unwrap().status(),
        AgentStatus::Failed { .. }
    ));
}

#[test]
fn unconfigured_provider_is_rejected_before_registering() {
    let mut config = Config::default();
    config.agents.remove(&ProviderId::OpenCode);
    let w = World::with_config(config);
    let s = w.open();
    let (f, wt) = new_feature(&s, "login");
    assert_eq!(
        code(s.spawn_agent(f, wt, ProviderId::OpenCode)),
        "PROVIDER_NOT_CONFIGURED"
    );
    assert!(s.project().agents().is_empty());
}

#[test]
fn stop_restart_and_remove_an_agent() {
    let w = World::new();
    let s = w.open();
    let (f, wt) = new_feature(&s, "login");
    let agent = s.spawn_agent(f, wt, ProviderId::Claude).unwrap();

    assert_eq!(code(s.remove_agent(agent)), "AGENT_STILL_RUNNING");
    s.stop_agent(agent).unwrap();
    assert!(!w.agents.is_alive(agent));
    s.restart_agent(agent).unwrap();
    assert!(w.agents.is_alive(agent));
    assert_eq!(w.agents.spawned().len(), 2);

    s.stop_agent(agent).unwrap();
    s.remove_agent(agent).unwrap();
    assert!(s.project().agents().is_empty());
}

#[test]
fn input_and_output_flow_through_the_session() {
    let w = World::new();
    let s = w.open();
    let (f, wt) = new_feature(&s, "login");
    let agent = s.spawn_agent(f, wt, ProviderId::Claude).unwrap();
    let sink = Arc::new(CollectingChunkSink::default());
    let _sub = s.attach_agent(agent, sink.clone()).unwrap();
    s.agent_input(agent, b"hola\r").unwrap();
    assert!(sink.output().contains("echo: hola"));
    s.agent_resize(agent, gryphus_core::ports::TermSize { cols: 90, rows: 30 })
        .unwrap();
    assert_eq!(w.agents.size(agent).unwrap().cols, 90);
}

#[test]
fn attaching_to_an_agent_without_process_yields_an_ended_stream() {
    let w = World::new();
    let s = w.open();
    let (f, wt) = new_feature(&s, "login");
    let agent = s.spawn_agent(f, wt, ProviderId::Claude).unwrap();
    s.close().unwrap();

    // Nueva sesión con un runtime nuevo: el proceso ya no existe.
    let w2 = World {
        agents: FakeAgentRuntime::new(),
        ..w
    };
    let s2 = w2.open();
    let sink = Arc::new(CollectingChunkSink::default());
    let _sub = s2.attach_agent(agent, sink.clone()).unwrap();
    assert!(matches!(
        sink.chunks()[..],
        [AgentChunk::Scrollback(_), AgentChunk::Exited(_)]
    ));
}

#[test]
fn close_stops_active_agents() {
    let w = World::new();
    let s = w.open();
    let (f, wt) = new_feature(&s, "login");
    let agent = s.spawn_agent(f, wt, ProviderId::Claude).unwrap();
    s.close().unwrap();
    assert!(!w.agents.is_alive(agent));
}

// ── Archivos y git ───────────────────────────────────────────────────────────

#[test]
fn file_operations_use_the_worktree_root() {
    let w = World::new();
    let s = w.open();
    let main = main_wt(&s);
    s.write_file(main, "src/a.rs", b"fn a() {}").unwrap();
    assert_eq!(
        s.read_file(main, "src/a.rs").unwrap(),
        FileContent::Text("fn a() {}".into())
    );
    let names: Vec<_> = s
        .list_dir(main, "")
        .unwrap()
        .into_iter()
        .map(|e| e.name)
        .collect();
    assert_eq!(names, ["src"]);
    assert_eq!(code(s.read_file(main, "../fuera")), "INVALID_PATH");
}

#[test]
fn disk_changes_publish_files_and_git_events() {
    let w = World::new();
    let s = w.open();
    let main = main_wt(&s);
    w.events.take();
    w.fs.write_raw("nuevo.txt", b"x");
    let events = w.events.take();
    assert!(events.contains(&DomainEvent::FilesChanged {
        worktree: main,
        paths: vec![gryphus_core::RelPath::parse("nuevo.txt").unwrap()],
    }));
    assert!(events.contains(&DomainEvent::GitChanged { worktree: main }));
}

#[test]
fn git_queries_resolve_the_worktree() {
    let w = World::new();
    let s = w.open();
    let (_, wt) = new_feature(&s, "login");
    let path = s.project().worktree(wt).unwrap().path().to_path_buf();
    w.git.write_file(&path, "a.txt", "a\n");

    let status = s.git_status(wt).unwrap();
    assert_eq!(status[0].state, FileState::Untracked);
    let diff = s
        .git_diff(wt, &DiffBase::MergeBase(BranchName::parse("main").unwrap()))
        .unwrap();
    assert_eq!(diff.files[0].path.as_str(), "a.txt");

    let overview = s.git_overview().unwrap();
    assert_eq!(overview.head.unwrap().as_str(), "main");
    assert!(overview
        .branches
        .iter()
        .any(|b| b.name.as_str() == "feat/login"));
}

#[test]
fn unknown_ids_are_reported() {
    let w = World::new();
    let s = w.open();
    let ghost_wt = WorktreeId::from_raw(999);
    assert_eq!(code(s.list_dir(ghost_wt, "")), "WORKTREE_NOT_FOUND");
    assert_eq!(
        code(s.stop_agent(gryphus_core::AgentId::from_raw(999))),
        "AGENT_NOT_FOUND"
    );
    assert_eq!(
        code(s.delete_feature(FeatureId::from_raw(999), false, false)),
        "FEATURE_NOT_FOUND"
    );
}
