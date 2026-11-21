//! Estado global: adaptadores compartidos y registro ventana ⇄ proyecto.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use gryphus_agents::PtyRuntime;
use gryphus_app::{Ports, ProjectSession};
use gryphus_core::ports::Subscription;
use gryphus_core::EventSink;
use gryphus_fs::LocalFs;
use gryphus_git::CliGit;
use gryphus_store::JsonStore;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// Qué proyecto muestra cada ventana. Un proyecto vive en una sola ventana.
#[derive(Default)]
pub struct Registry {
    by_label: HashMap<String, ProjectSession>,
}

impl Registry {
    pub fn session(&self, label: &str) -> Option<ProjectSession> {
        self.by_label.get(label).cloned()
    }

    /// Ventana que ya muestra el proyecto con esta raíz.
    pub fn label_for_root(&self, root: &Path) -> Option<String> {
        self.by_label
            .iter()
            .find(|(_, s)| s.root() == root)
            .map(|(label, _)| label.clone())
    }

    pub fn bind(&mut self, label: &str, session: ProjectSession) {
        self.by_label.insert(label.to_owned(), session);
    }

    pub fn remove(&mut self, label: &str) -> Option<ProjectSession> {
        self.by_label.remove(label)
    }
}

pub struct AppState {
    pub registry: Mutex<Registry>,
    /// Salida de agente conectada a la terminal de cada ventana (una por ventana).
    pub attachments: Mutex<HashMap<String, Subscription>>,
    /// Ventanas cuyo cierre debe confirmar la UI (ver `cmd::SET_CLOSE_GUARD`).
    pub close_guards: Mutex<HashSet<String>>,
    pub git: Arc<CliGit>,
    pub fs: Arc<LocalFs>,
    pub store: Arc<JsonStore>,
}

impl AppState {
    pub fn new(data_dir: &Path, config_file: PathBuf) -> Self {
        Self {
            registry: Mutex::new(Registry::default()),
            attachments: Mutex::new(HashMap::new()),
            close_guards: Mutex::new(HashSet::new()),
            git: Arc::new(CliGit::new()),
            fs: Arc::new(LocalFs::new()),
            store: Arc::new(JsonStore::new(data_dir, config_file)),
        }
    }

    /// Cada sesión tiene su propio runtime de agentes: los ids de agente son
    /// secuenciales por proyecto y chocarían entre proyectos.
    pub fn ports(&self, events: Arc<dyn EventSink>) -> Ports {
        Ports {
            git: self.git.clone(),
            agents: Arc::new(PtyRuntime::new()),
            fs: self.fs.clone(),
            store: self.store.clone(),
            events,
        }
    }

    pub fn registry(&self) -> MutexGuard<'_, Registry> {
        lock(&self.registry)
    }

    pub fn attach(&self, label: &str, sub: Subscription) {
        lock(&self.attachments).insert(label.to_owned(), sub);
    }

    pub fn detach(&self, label: &str) {
        lock(&self.attachments).remove(label);
    }

    pub fn set_close_guard(&self, label: &str, guard: bool) {
        let mut guards = lock(&self.close_guards);
        if guard {
            guards.insert(label.to_owned());
        } else {
            guards.remove(label);
        }
    }

    pub fn close_guarded(&self, label: &str) -> bool {
        lock(&self.close_guards).contains(label)
    }

    /// Al destruirse una ventana: suelta su terminal, detiene sus agentes y guarda.
    pub fn close_window(&self, label: &str) {
        self.set_close_guard(label, false);
        self.detach(label);
        let session = self.registry().remove(label);
        if let Some(session) = session {
            let _ = session.close();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gryphus_testkit::{FakeAgentRuntime, FakeFs, FakeGit, FakeStore, RecordingEventSink};

    fn session(root: &str) -> ProjectSession {
        let git = FakeGit::new(root);
        git.write_file(Path::new(root), "README.md", "x");
        git.commit_all(Path::new(root), "init");
        let ports = Ports {
            git: Arc::new(git),
            agents: Arc::new(FakeAgentRuntime::new()),
            fs: Arc::new(FakeFs::new(root)),
            store: Arc::new(FakeStore::default()),
            events: Arc::new(RecordingEventSink::default()),
        };
        ProjectSession::open(Path::new(root), ports).unwrap()
    }

    #[test]
    fn one_project_per_window_and_lookup_by_root() {
        let mut r = Registry::default();
        assert!(r.session("main").is_none());
        r.bind("main", session("/a"));
        r.bind("project-b", session("/b"));
        assert_eq!(r.label_for_root(Path::new("/a")).as_deref(), Some("main"));
        assert_eq!(
            r.label_for_root(Path::new("/b")).as_deref(),
            Some("project-b")
        );
        assert_eq!(r.label_for_root(Path::new("/c")), None);
        assert!(r.remove("main").is_some());
        assert_eq!(r.label_for_root(Path::new("/a")), None);
    }
}
