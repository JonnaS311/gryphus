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
    wait_until, CollectingChunkSink, FakeAgentRuntime, FakeFs, FakeGit, FakeStore,
    RecordingEventSink,
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

    /// La app se reinició: mismos repo y datos, ningún proceso de agente vivo.
    fn restarted(self) -> Self {
        Self {
            agents: FakeAgentRuntime::new(),
            ..self
        }
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
fn reopening_restores_features_and_relaunches_active_agents() {
    let w = World::new();
    let s = w.open();
    let (f, wt) = new_feature(&s, "login");
    let agent = s.spawn_agent(f, wt, ProviderId::Claude).unwrap();
    let stopped = s.spawn_agent(f, wt, ProviderId::Claude).unwrap();
    s.stop_agent(stopped).unwrap();
    // Sin `close`: como si la app se hubiera caído.
    drop(s);

    let w = w.restarted();
    let reopened = w.open();
    let p = reopened.project();
    assert_eq!(p.features()[0].name().as_str(), "login");
    assert_eq!(p.agent(agent).unwrap().status(), &AgentStatus::Running);
    assert!(w.agents.is_alive(agent));
    assert_eq!(
        p.agent(stopped).unwrap().status(),
        &AgentStatus::Exited { code: None },
        "el que estaba detenido sigue detenido"
    );
    assert_eq!(w.agents.spawned().len(), 1);
}

#[test]
fn agents_active_at_close_resume_their_claude_conversation() {
    let w = World::new();
    let s = w.open();
    let (f, wt) = new_feature(&s, "login");
    let agent = s.spawn_agent(f, wt, ProviderId::Claude).unwrap();
    let args = w.agents.spawned()[0].args.clone();
    assert_eq!(args[0], "--session-id");
    let id = args[1].clone();
    assert_eq!(id.len(), 36, "UUID: {id}");
    let cwd = s.project().worktree(wt).unwrap().path().to_path_buf();
    s.close().unwrap();

    // El CLI guardó la conversación: se retoma.
    let w = w.restarted();
    w.agents.save_session(ProviderId::Claude, &cwd, &id);
    let reopened = w.open();
    assert_eq!(w.agents.spawned()[0].args, ["--resume", id.as_str()]);
    assert_eq!(
        reopened.project().agent(agent).unwrap().status(),
        &AgentStatus::Running
    );
    reopened.close().unwrap();

    // Sin conversación guardada (nunca se le escribió) empieza una nueva.
    let fresh = w.restarted();
    let _s = fresh.open();
    let args = &fresh.agents.spawned()[0].args;
    assert_eq!(args[0], "--session-id");
    assert_ne!(args[1], id);
}

#[test]
fn codex_conversation_is_discovered_and_resumed_on_restart() {
    let w = World::new();
    let s = w.open();
    let (f, wt) = new_feature(&s, "login");
    let cwd = s.project().worktree(wt).unwrap().path().to_path_buf();
    let first = s.spawn_agent(f, wt, ProviderId::Codex).unwrap();
    let second = s.spawn_agent(f, wt, ProviderId::Codex).unwrap();
    assert!(w.agents.spawned().iter().all(|spec| spec.args.is_empty()));
    w.agents.save_session(ProviderId::Codex, &cwd, "codex-1");
    w.agents.save_session(ProviderId::Codex, &cwd, "codex-2");

    // Al terminar cada uno se anota la suya, sin repetir.
    s.stop_agent(first).unwrap();
    s.stop_agent(second).unwrap();
    let id = |a| {
        s.project()
            .agent(a)
            .unwrap()
            .session()
            .and_then(|x| x.id.clone())
    };
    assert_eq!(id(first).as_deref(), Some("codex-1"));
    assert_eq!(id(second).as_deref(), Some("codex-2"));

    s.restart_agent(second).unwrap();
    assert_eq!(w.agents.spawned()[2].args, ["resume", "codex-2"]);
}

#[test]
fn opencode_keeps_its_session_id_across_launches() {
    let w = World::new();
    let s = w.open();
    let (f, wt) = new_feature(&s, "login");
    let agent = s.spawn_agent(f, wt, ProviderId::OpenCode).unwrap();
    let args = w.agents.spawned()[0].args.clone();
    assert_eq!(args[0], "--session");
    assert!(args[1].starts_with("ses_"), "{args:?}");
    let cwd = s.project().worktree(wt).unwrap().path().to_path_buf();
    w.agents.save_session(ProviderId::OpenCode, &cwd, &args[1]);
    s.stop_agent(agent).unwrap();
    s.restart_agent(agent).unwrap();
    assert_eq!(w.agents.spawned()[1].args, args);
}

#[test]
fn spawning_a_new_agent_never_resumes() {
    let w = World::new();
    let s = w.open();
    let (f, wt) = new_feature(&s, "login");
    let cwd = s.project().worktree(wt).unwrap().path().to_path_buf();
    w.agents.save_session(ProviderId::Codex, &cwd, "vieja");
    s.spawn_agent(f, wt, ProviderId::Codex).unwrap();
    assert!(w.agents.spawned()[0].args.is_empty());
}

#[test]
fn agent_whose_worktree_is_gone_is_not_relaunched() {
    let w = World::new();
    let s = w.open();
    let (f, wt) = new_feature(&s, "login");
    let agent = s.spawn_agent(f, wt, ProviderId::Claude).unwrap();
    let path = s.project().worktree(wt).unwrap().path().to_path_buf();
    s.close().unwrap();
    w.git.remove_worktree(Path::new(ROOT), &path, true).unwrap();

    let w = w.restarted();
    let reopened = w.open();
    assert_eq!(
        reopened.project().agent(agent).unwrap().status(),
        &AgentStatus::Exited { code: None }
    );
    assert!(w.agents.spawned().is_empty());
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

/// Un worktree que ya faltaba (borrado fuera de Gryphus) no debe quedarse en el
/// registro tras borrar su feature.
#[test]
fn deleting_a_feature_forgets_its_already_missing_worktrees() {
    let w = World::new();
    let s = w.open();
    let (f, wt) = new_feature(&s, "login");
    let path = s.project().worktree(wt).unwrap().path().to_path_buf();
    w.git.remove_worktree(Path::new(ROOT), &path, true).unwrap();
    s.refresh_worktrees().unwrap();
    assert!(s.project().worktree(wt).unwrap().is_missing());

    s.delete_feature(f, false, false).unwrap();
    assert!(s.project().worktree(wt).is_err(), "olvidado");
    assert_eq!(w.saved(), s.project());
}

#[test]
fn deleting_a_feature_whose_worktree_was_deleted_by_hand_just_works() {
    let w = World::new();
    let s = w.open();
    let (f, wt) = new_feature(&s, "login");
    let path = s.project().worktree(wt).unwrap().path().to_path_buf();
    // Sin refrescar: la sesión aún cree que el worktree existe.
    w.git.delete_worktree_dir(&path);

    s.delete_feature(f, true, false).unwrap();
    assert!(s.project().feature(f).is_err());
    assert!(s.project().worktree(wt).is_err(), "olvidado");
    assert!(w.events.events().contains(&DomainEvent::FeatureRemoved(f)));
    assert_eq!(w.saved(), s.project());
}

#[test]
fn recreating_a_feature_whose_worktree_was_deleted_by_hand_reuses_its_branch() {
    let w = World::new();
    let s = w.open();
    let (f, wt) = new_feature(&s, "login");
    let path = s.project().worktree(wt).unwrap().path().to_path_buf();
    w.git.write_file(&path, "trabajo.txt", "x\n");
    w.git.commit_all(&path, "trabajo");
    s.delete_feature(f, false, false).unwrap();
    w.git.delete_worktree_dir(&path);

    let (_, wt) = new_feature(&s, "login");
    let p = s.project();
    let worktree = p.worktree(wt).unwrap();
    assert_eq!(worktree.branch().unwrap().as_str(), "feat/login");
    assert_eq!(worktree.path(), path);
    assert_eq!(
        w.git.status(&path).unwrap(),
        [],
        "conserva el trabajo de la branch"
    );
    assert!(w
        .git
        .diff(
            &path,
            &DiffBase::MergeBase(BranchName::parse("main").unwrap())
        )
        .unwrap()
        .files
        .iter()
        .any(|f| f.path.as_str() == "trabajo.txt"));
}

#[test]
fn a_branch_in_use_by_a_live_worktree_is_still_rejected() {
    let w = World::new();
    let s = w.open();
    let (f, _) = new_feature(&s, "login");
    s.delete_feature(f, false, false).unwrap();
    let r = s.create_feature("login", WorktreeChoice::New { base: None });
    assert_eq!(code(r), "ALREADY_EXISTS");
}

#[test]
fn writing_inside_dot_git_is_rejected() {
    let w = World::new();
    let s = w.open();
    let main = main_wt(&s);
    assert_eq!(
        code(s.write_file(main, ".git/hooks/pre-commit", b"x")),
        "INVALID_PATH"
    );
    assert_eq!(
        code(s.write_file(main, ".GIT/config", b"x")),
        "INVALID_PATH"
    );
    s.write_file(main, "docs/.git-notas.md", b"ok").unwrap();
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
    // Primero la conversación (ver `agents_active_at_close_resume_…`), luego la config.
    assert_eq!(spec.args[0], "--session-id");
    assert_eq!(spec.args[2..], ["--modelo", "x"]);
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
fn agent_that_goes_quiet_after_an_order_is_reported_idle_once() {
    let w = World::new();
    let s = w.open();
    let (f, wt) = new_feature(&s, "login");
    let agent = s.spawn_agent(f, wt, ProviderId::Claude).unwrap();
    let idle = || {
        w.events
            .events()
            .iter()
            .filter(|e| **e == DomainEvent::AgentIdle { agent })
            .count()
    };

    // Salida sin orden previa (arranque, tecleo) no cuenta.
    s.agent_input(agent, b"tecleando").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(600));
    assert_eq!(idle(), 0);

    s.agent_input(agent, b"hola\r").unwrap();
    assert!(wait_until(std::time::Duration::from_secs(8), || idle() == 1));
    std::thread::sleep(std::time::Duration::from_secs(1));
    assert_eq!(idle(), 1, "se avisa una sola vez por orden");
}

#[test]
fn terminal_bell_reports_the_agent_idle_at_once() {
    let w = World::new();
    let s = w.open();
    let (f, wt) = new_feature(&s, "login");
    let agent = s.spawn_agent(f, wt, ProviderId::Claude).unwrap();
    s.agent_input(agent, b"\x07\r").unwrap();
    assert!(w
        .events
        .events()
        .contains(&DomainEvent::AgentIdle { agent }));
}

#[test]
fn attaching_to_an_agent_without_process_yields_an_ended_stream() {
    let w = World::new();
    let s = w.open();
    let (f, wt) = new_feature(&s, "login");
    let agent = s.spawn_agent(f, wt, ProviderId::Claude).unwrap();
    // Detenido antes de cerrar: al reabrir no se relanza.
    s.stop_agent(agent).unwrap();
    s.close().unwrap();

    // Nueva sesión con un runtime nuevo: el proceso ya no existe.
    let w2 = w.restarted();
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
fn move_files_between_directories() {
    let w = World::new();
    let s = w.open();
    let main = main_wt(&s);
    s.write_file(main, "a.txt", b"a").unwrap();
    s.write_file(main, "src/b.rs", b"b").unwrap();
    s.write_file(main, "docs/a.txt", b"otro").unwrap();

    // A una carpeta y de vuelta a la raíz, conservando el nombre.
    assert_eq!(
        s.move_path(main, "src/b.rs", "docs").unwrap().as_str(),
        "docs/b.rs"
    );
    assert_eq!(s.move_path(main, "docs/b.rs", "").unwrap().as_str(), "b.rs");
    // Una carpeta entera.
    assert_eq!(
        s.move_path(main, "src", "docs").unwrap().as_str(),
        "docs/src"
    );
    // Soltar en su propio directorio no hace nada.
    assert_eq!(s.move_path(main, "a.txt", "").unwrap().as_str(), "a.txt");
    // Nunca sobrescribe, ni mete una carpeta dentro de sí misma, ni entra en `.git`.
    assert_eq!(code(s.move_path(main, "a.txt", "docs")), "ALREADY_EXISTS");
    assert!(s.move_path(main, "docs", "docs/src").is_err());
    assert_eq!(code(s.move_path(main, "a.txt", ".git")), "INVALID_PATH");
    assert!(s.read_file(main, "a.txt").is_ok());
}

#[test]
fn rename_and_paste_files() {
    let w = World::new();
    let s = w.open();
    let main = main_wt(&s);
    s.write_file(main, "a.txt", b"a").unwrap();
    s.write_file(main, "src/b.rs", b"b").unwrap();

    // Renombrar en la raíz y dentro de un directorio.
    assert_eq!(
        s.rename_path(main, "a.txt", " c.txt ").unwrap().as_str(),
        "c.txt"
    );
    assert_eq!(
        s.rename_path(main, "src/b.rs", "d.rs").unwrap().as_str(),
        "src/d.rs"
    );
    assert_eq!(
        code(s.rename_path(main, "c.txt", "x/y.txt")),
        "INVALID_PATH"
    );
    assert_eq!(code(s.rename_path(main, "c.txt", "src")), "ALREADY_EXISTS");

    // Pegar: en la raíz choca con el original → `copia`; en otro directorio conserva el nombre.
    assert_eq!(
        s.copy_path(main, "c.txt", main, "").unwrap().as_str(),
        "c copia.txt"
    );
    assert_eq!(
        s.copy_path(main, "c.txt", main, "").unwrap().as_str(),
        "c copia 2.txt"
    );
    assert_eq!(
        s.copy_path(main, "c.txt", main, "src").unwrap().as_str(),
        "src/c.txt"
    );
    assert_eq!(
        s.read_file(main, "c copia.txt").unwrap(),
        FileContent::Text("a".into())
    );
}

#[test]
fn importing_requires_absolute_paths() {
    let w = World::new();
    let s = w.open();
    let main = main_wt(&s);
    let rel = [std::path::PathBuf::from("a.txt")];
    assert_eq!(code(s.import_paths(main, &rel, "")), "INVALID_PATH");
    assert_eq!(code(s.import_paths(main, &[], "../x")), "INVALID_PATH");
    assert!(s.import_paths(main, &[], "").unwrap().is_empty());
}

/// Las rutas de `FakeFs` (`/repo/…`) solo son absolutas fuera de Windows.
#[cfg(unix)]
#[test]
fn importing_uses_copy_names() {
    let w = World::new();
    let s = w.open();
    let main = main_wt(&s);
    s.write_file(main, "a.txt", b"a").unwrap();
    s.write_file(main, "src/b.rs", b"b").unwrap();
    let sources = [
        std::path::PathBuf::from(ROOT).join("a.txt"),
        std::path::PathBuf::from(ROOT).join("a.txt"),
    ];
    let created = s.import_paths(main, &sources, "").unwrap();
    let created: Vec<_> = created.iter().map(|p| p.as_str()).collect();
    assert_eq!(created, ["a copia.txt", "a copia 2.txt"]);
    let into_src = s.import_paths(main, &sources[..1], "src").unwrap();
    assert_eq!(into_src[0].as_str(), "src/a.txt");
}

#[test]
fn agents_are_named_and_renamed() {
    let w = World::new();
    let s = w.open();
    let (f, wt) = new_feature(&s, "login");
    let agent = s.spawn_agent(f, wt, ProviderId::Claude).unwrap();
    assert!(s.project().agent(agent).unwrap().name().is_some());
    s.rename_agent(agent, "  Revisor ").unwrap();
    assert_eq!(
        w.saved().agent(agent).unwrap().name().unwrap().as_str(),
        "Revisor"
    );
    assert_eq!(code(s.rename_agent(agent, "   ")), "INVALID_NAME");
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

/// Un cambio dentro del `.git` del principal (p. ej. `git worktree add` desde una
/// terminal) relee los worktrees y avisa del estado git de todos.
#[test]
fn git_internal_change_in_main_refreshes_worktrees_and_all_git_states() {
    let w = World::new();
    let s = w.open();
    let main = main_wt(&s);
    let (_, feature_wt) = new_feature(&s, "login");
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
    w.events.take();

    w.fs.emit_changed(&[".git"]);

    let events = w.events.take();
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, DomainEvent::FilesChanged { .. })),
        ".git no es un archivo del explorador: {events:?}"
    );
    assert!(events.contains(&DomainEvent::WorktreesChanged));
    let externo = s
        .project()
        .worktree_by_path(Path::new("/externo"))
        .expect("el worktree externo se detecta")
        .id();
    for wt in [main, feature_wt, externo] {
        assert!(
            events.contains(&DomainEvent::GitChanged { worktree: wt }),
            "falta GitChanged de {wt:?}: {events:?}"
        );
    }
}

#[test]
fn git_internal_change_with_files_publishes_both() {
    let w = World::new();
    let s = w.open();
    let main = main_wt(&s);
    w.events.take();
    w.fs.emit_changed(&[".git", "src/a.rs"]);
    let events = w.events.take();
    assert!(events.contains(&DomainEvent::FilesChanged {
        worktree: main,
        paths: vec![gryphus_core::RelPath::parse("src/a.rs").unwrap()],
    }));
    assert!(events.contains(&DomainEvent::GitChanged { worktree: main }));
}

/// Lo ignorado por `.gitignore` actualiza el explorador, pero no relanza git.
#[test]
fn gitignored_changes_publish_files_but_not_git() {
    let w = World::new();
    let s = w.open();
    let main = main_wt(&s);
    w.events.take();
    w.fs.emit_ignored(&["target/debug/app"]);
    let events = w.events.take();
    assert_eq!(
        events,
        [DomainEvent::FilesChanged {
            worktree: main,
            paths: vec![gryphus_core::RelPath::parse("target/debug/app").unwrap()],
        }]
    );
}

#[test]
fn closed_session_stops_watching() {
    let w = World::new();
    let s = w.open();
    s.close().unwrap();
    w.events.take();
    w.fs.emit_changed(&["a.txt"]);
    w.fs.emit_ignored(&["target/x"]);
    let events = w.events.take();
    assert!(events.is_empty(), "{events:?}");
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
