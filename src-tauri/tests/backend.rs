//! Backend completo con adaptadores reales (sin UI): git real, disco real,
//! PTY real con `fake-agent`, persistencia real. Es el flujo principal de
//! Gryphus de punta a punta por debajo del IPC.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use gryphus_agents::PtyRuntime;
use gryphus_app::{Ports, ProjectSession, WorktreeChoice};
use gryphus_core::ports::{DiffBase, DiffLine, FileContent, FileState, GitPort};
use gryphus_core::{AgentStatus, BranchName, DomainEvent, ProviderId};
use gryphus_fs::LocalFs;
use gryphus_git::CliGit;
use gryphus_store::JsonStore;
use gryphus_testkit::{
    fake_agent_bin, wait_until, CollectingChunkSink, RecordingEventSink, TempRepo,
};

const T: Duration = Duration::from_secs(10);

struct Env {
    repo: TempRepo,
    git: Arc<CliGit>,
    store: Arc<JsonStore>,
    events: Arc<RecordingEventSink>,
}

impl Env {
    fn new() -> Self {
        let repo = TempRepo::new();
        let git = repo
            .git_env()
            .into_iter()
            .fold(CliGit::new(), |g, (k, v)| g.env(k, v));
        let data = repo.base().join("appdata");
        let config = data.join("config.toml");
        std::fs::create_dir_all(&data).unwrap();
        // Los tres proveedores apuntan a fake-agent (TOML con cadenas literales).
        let bin = fake_agent_bin();
        let toml: String = ["opencode", "claude", "codex"]
            .iter()
            .map(|p| format!("[agents.{p}]\ncommand = '{}'\n\n", bin.display()))
            .collect();
        std::fs::write(&config, toml).unwrap();
        Self {
            git: Arc::new(git),
            store: Arc::new(JsonStore::new(&data, config)),
            events: Arc::new(RecordingEventSink::default()),
            repo,
        }
    }

    fn open(&self) -> ProjectSession {
        let ports = Ports {
            git: self.git.clone(),
            agents: Arc::new(PtyRuntime::new()),
            fs: Arc::new(LocalFs::new()),
            store: self.store.clone(),
            events: self.events.clone(),
        };
        ProjectSession::open(self.repo.root(), ports).unwrap()
    }
}

#[test]
fn feature_agent_files_and_git_end_to_end() {
    let env = Env::new();
    let session = env.open();

    // 1. Feature con worktree nuevo junto al repo.
    let feature = session
        .create_feature("login", WorktreeChoice::New { base: None })
        .unwrap();
    let wt = session.project().feature(feature).unwrap().worktrees()[0];
    let wt_path = session.project().worktree(wt).unwrap().path().to_path_buf();
    assert!(wt_path.join("README.md").exists(), "worktree en disco");
    // canonicalize: en CI de Windows el temporal usa nombres 8.3 (RUNNER~1) y git
    // devuelve la ruta larga.
    assert!(wt_path
        .canonicalize()
        .unwrap()
        .starts_with(env.repo.base().canonicalize().unwrap()));

    // 2. Agente (fake-agent en ConPTY) que escribe un archivo en su worktree.
    let agent = session
        .spawn_agent(feature, wt, ProviderId::Claude)
        .unwrap();
    let sink = Arc::new(CollectingChunkSink::default());
    let _sub = session.attach_agent(agent, sink.clone()).unwrap();
    assert!(wait_until(T, || sink.output().contains("fake-agent ready")));
    session
        .agent_input(agent, b"::write src/nuevo.txt hola\r")
        .unwrap();
    assert!(wait_until(T, || sink
        .output()
        .contains("wrote src/nuevo.txt")));

    // 3. El watcher lo detecta y lo publica.
    let saw_file = || {
        env.events.events().iter().any(|e| {
            matches!(e, DomainEvent::FilesChanged { worktree, paths }
                if *worktree == wt && paths.iter().any(|p| p.as_str() == "src/nuevo.txt"))
        })
    };
    assert!(wait_until(T, saw_file), "{:?}", env.events.events());

    // 4. Git lo ve en status y en el diff de la feature.
    let status = session.git_status(wt).unwrap();
    assert!(status
        .iter()
        .any(|s| s.path.as_str() == "src/nuevo.txt" && s.state == FileState::Untracked));
    let diff = session
        .git_diff(wt, &DiffBase::MergeBase(BranchName::parse("main").unwrap()))
        .unwrap();
    let file = diff
        .files
        .iter()
        .find(|f| f.path.as_str() == "src/nuevo.txt")
        .unwrap();
    assert_eq!(file.hunks[0].lines, [DiffLine::Added("hola".into())]);
    assert_eq!(
        session.read_file(wt, "src/nuevo.txt").unwrap(),
        FileContent::Text("hola".into())
    );

    // 5. Parar el agente y borrar la feature con su worktree (tiene cambios: force).
    session.stop_agent(agent).unwrap();
    assert!(wait_until(T, || {
        session
            .project()
            .agent(agent)
            .is_ok_and(|a| !a.status().is_active())
    }));
    assert_eq!(
        session
            .delete_feature(feature, true, false)
            .unwrap_err()
            .code(),
        "CONFLICT"
    );
    session.delete_feature(feature, true, true).unwrap();
    assert!(!wt_path.exists(), "worktree eliminado del disco");
    let branches = env.git.list_branches(env.repo.root()).unwrap();
    assert!(
        branches.iter().any(|b| b.name.as_str() == "feat/login"),
        "la branch se conserva"
    );

    // 6. Persistencia: al reabrir, la feature ya no está.
    session.close().unwrap();
    let reopened = env.open();
    assert!(reopened.project().features().is_empty());
}

#[test]
fn agents_are_relaunched_after_reopening() {
    let env = Env::new();
    let session = env.open();
    let feature = session
        .create_feature("api", WorktreeChoice::New { base: None })
        .unwrap();
    let wt = session.project().feature(feature).unwrap().worktrees()[0];
    let agent = session.spawn_agent(feature, wt, ProviderId::Codex).unwrap();
    let stopped = session
        .spawn_agent(feature, wt, ProviderId::Claude)
        .unwrap();
    session.stop_agent(stopped).unwrap();
    assert!(wait_until(T, || {
        session
            .project()
            .agent(stopped)
            .is_ok_and(|a| !a.status().is_active())
    }));
    session.close().unwrap();
    drop(session);

    // El que seguía activo al cerrar se relanza solo; el detenido sigue detenido.
    let reopened = env.open();
    let sink = Arc::new(CollectingChunkSink::default());
    let _sub = reopened.attach_agent(agent, sink.clone()).unwrap();
    assert!(wait_until(T, || sink.output().contains("fake-agent ready")));
    assert_eq!(
        reopened.project().agent(agent).unwrap().status(),
        &AgentStatus::Running
    );
    assert!(!reopened
        .project()
        .agent(stopped)
        .unwrap()
        .status()
        .is_active());

    // Y el detenido se puede relanzar a mano.
    reopened.restart_agent(stopped).unwrap();
    let sink = Arc::new(CollectingChunkSink::default());
    let _sub = reopened.attach_agent(stopped, sink.clone()).unwrap();
    assert!(wait_until(T, || sink.output().contains("fake-agent ready")));
    reopened.close().unwrap();
}

#[test]
fn external_worktrees_are_discovered() {
    let env = Env::new();
    let session = env.open();
    let path = env.repo.add_worktree("externo", "externo");
    // El cambio en .git del principal dispara la resincronización.
    assert!(wait_until(T, || session
        .project()
        .worktree_by_path(&path)
        .is_some()
        || session.refresh_worktrees().unwrap_or(false)));
    assert!(session
        .project()
        .worktrees()
        .iter()
        .any(|w| same(w.path(), &path)));
}

fn same(a: &Path, b: &Path) -> bool {
    a == b || a.canonicalize().ok() == b.canonicalize().ok()
}
