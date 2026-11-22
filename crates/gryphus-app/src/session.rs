//! [`ProjectSession`]: casos de uso de un proyecto abierto.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::time::Instant;

use gryphus_core::ports::{
    AgentChunk, AgentExit, AgentRuntimePort, Branch, ChunkSink, Diff, DiffBase, DirEntry,
    FileContent, FileStatus, FsPort, GitPort, NewWorktree, PortError, PortErrorKind, SpawnSpec,
    StorePort, Subscription, TermSize,
};
use gryphus_core::{
    AgentId, AgentName, AgentStatus, BranchName, Config, DomainError, DomainEvent, EventSink,
    FeatureId, FeatureName, PathError, Project, ProjectId, ProviderId, RelPath, WorktreeId,
};

use crate::activity::{Activity, ActivitySink, POLL_EVERY};
use crate::error::AppResult;
use crate::watch::WatchSink;

/// Implementaciones de los puertos que usa una sesión (las inyecta `src-tauri`).
#[derive(Clone)]
pub struct Ports {
    pub git: Arc<dyn GitPort>,
    pub agents: Arc<dyn AgentRuntimePort>,
    pub fs: Arc<dyn FsPort>,
    pub store: Arc<dyn StorePort>,
    pub events: Arc<dyn EventSink>,
}

/// Worktree para una feature nueva.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorktreeChoice {
    /// Crear `feat/<nombre>` desde `base` (o desde HEAD) en la ruta de la config.
    New {
        base: Option<String>,
    },
    Existing(WorktreeId),
}

/// Resumen git para la barra de estado y el panel izquierdo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitOverview {
    pub head: Option<BranchName>,
    pub branches: Vec<Branch>,
}

pub(crate) struct Inner {
    root: PathBuf,
    ports: Ports,
    config: Config,
    project: Mutex<Project>,
    /// Ordena los guardados: se toma antes que `project` y se suelta tras guardar,
    /// así una instantánea antigua nunca se escribe después de una más reciente.
    save: Mutex<()>,
    watches: Mutex<HashMap<WorktreeId, Subscription>>,
    /// Detección de "terminó y espera instrucciones" de los agentes vivos (ver `activity`).
    activity: Mutex<HashMap<AgentId, Activity>>,
    /// Suscripciones internas a la salida de cada agente vivo.
    monitors: Mutex<HashMap<AgentId, Subscription>>,
}

/// Estado vivo de un proyecto. Clonar es barato (comparte el estado).
#[derive(Clone)]
pub struct ProjectSession {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for ProjectSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProjectSession")
            .field("root", &self.inner.root)
            .finish_non_exhaustive()
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl ProjectSession {
    /// Abre (o crea) el proyecto del repo que contiene `path`.
    pub fn open(path: &Path, ports: Ports) -> AppResult<Self> {
        let info = ports.git.repo_info(path)?;
        let root = info.root;
        let config = ports.store.load_config()?;
        let id = ProjectId::from_canonical_root(&root);
        let mut project = ports
            .store
            .load_project(&id)?
            .unwrap_or_else(|| Project::new(root.clone()));
        project.reset_after_restart();
        project.name_unnamed_agents();
        project.sync_worktrees(&ports.git.list_worktrees(&root)?);
        ports.store.save_project(&project)?;

        let inner = Arc::new(Inner {
            root,
            ports,
            config,
            project: Mutex::new(project),
            save: Mutex::new(()),
            watches: Mutex::new(HashMap::new()),
            activity: Mutex::new(HashMap::new()),
            monitors: Mutex::new(HashMap::new()),
        });
        inner.ensure_watches();
        spawn_idle_ticker(Arc::downgrade(&inner));
        Ok(Self { inner })
    }

    pub fn root(&self) -> &Path {
        &self.inner.root
    }

    pub fn config(&self) -> &Config {
        &self.inner.config
    }

    /// Copia del estado actual (para mostrarlo).
    pub fn project(&self) -> Project {
        lock(&self.inner.project).clone()
    }

    // ── Features ─────────────────────────────────────────────────────────────

    pub fn create_feature(&self, name: &str, choice: WorktreeChoice) -> AppResult<FeatureId> {
        let name = FeatureName::parse(name)?;
        let inner = &self.inner;
        let duplicate = lock(&inner.project)
            .features()
            .iter()
            .any(|f| f.name().as_str().eq_ignore_ascii_case(name.as_str()));
        if duplicate {
            return Err(DomainError::DuplicateFeatureName(name.as_str().to_owned()).into());
        }

        // Ruta del worktree que crea este caso de uso (para deshacer si algo falla).
        let mut created: Option<PathBuf> = None;
        let worktree = match choice {
            WorktreeChoice::Existing(id) => id,
            WorktreeChoice::New { base } => {
                let path = inner.config.worktree_path(&inner.root, &name);
                let branch = BranchName::for_feature(&name);
                // Un worktree borrado a mano deja su registro en git y su branch:
                // se limpia el registro y, si la branch quedó libre, se reutiliza
                // (p. ej. al recrear una feature cuyo worktree ya no existe).
                inner.ports.git.prune_worktrees(&inner.root)?;
                let reuse_branch = inner
                    .ports
                    .git
                    .list_branches(&inner.root)?
                    .iter()
                    .any(|b| !b.is_remote && b.name == branch && !b.checked_out);
                let info = inner.ports.git.add_worktree(
                    &inner.root,
                    &NewWorktree {
                        path,
                        branch,
                        create_branch: !reuse_branch,
                        base: if reuse_branch { None } else { base },
                    },
                )?;
                created = Some(info.path.clone());
                let registered = inner.refresh_worktrees().and_then(|_| {
                    lock(&inner.project)
                        .worktree_by_path(&info.path)
                        .map(|w| w.id())
                        .ok_or_else(|| {
                            DomainError::Invariant("el worktree creado no aparece en git".into())
                                .into()
                        })
                });
                match registered {
                    Ok(id) => id,
                    Err(e) => {
                        inner.rollback_worktree(&info.path);
                        return Err(e);
                    }
                }
            }
        };

        match inner.mutate(|p| p.create_feature(name, worktree)) {
            Ok(feature) => {
                inner.publish(DomainEvent::FeatureChanged(feature));
                Ok(feature)
            }
            Err(e) => {
                if let Some(path) = created {
                    inner.rollback_worktree(&path);
                }
                Err(e)
            }
        }
    }

    /// Elimina la feature. Con `remove_worktrees` borra también sus worktrees en git
    /// (nunca el principal); `force` permite borrarlos con cambios sin commit.
    pub fn delete_feature(
        &self,
        feature: FeatureId,
        remove_worktrees: bool,
        force: bool,
    ) -> AppResult<()> {
        let inner = &self.inner;
        // Valida antes de tocar git: un worktree borrado a mano no es un error.
        lock(&inner.project).feature(feature)?;
        if remove_worktrees {
            // Olvida en git los worktrees cuyo directorio ya no existe y los marca
            // como ausentes aquí, para no intentar consultarlos ni borrarlos.
            inner.ports.git.prune_worktrees(&inner.root)?;
            inner.refresh_worktrees()?;
        }
        let (owned, to_remove): (Vec<WorktreeId>, Vec<(WorktreeId, PathBuf)>) = {
            let p = lock(&inner.project);
            let mut probe = p.clone();
            let removed = probe.delete_feature(feature)?;
            let worktrees: Vec<_> = removed
                .worktrees()
                .iter()
                .filter_map(|id| p.worktree(*id).ok())
                .filter(|w| !w.is_main())
                .collect();
            (
                worktrees.iter().map(|w| w.id()).collect(),
                worktrees
                    .iter()
                    .filter(|w| !w.is_missing())
                    .map(|w| (w.id(), w.path().to_path_buf()))
                    .collect(),
            )
        };
        if remove_worktrees {
            // Sin `force`, comprueba TODOS antes de borrar ninguno: si el segundo
            // tenía cambios, el primero ya estaba borrado y la feature quedaba a medias.
            if !force {
                for (_, path) in &to_remove {
                    if !inner.ports.git.status(path)?.is_empty() {
                        return Err(PortError::new(
                            PortErrorKind::Conflict,
                            format!("{} contains modified or untracked files", path.display()),
                        )
                        .into());
                    }
                }
            }
            for (_, path) in &to_remove {
                inner.ports.git.remove_worktree(&inner.root, path, force)?;
            }
        }
        inner.mutate(|p| p.delete_feature(feature))?;
        if remove_worktrees {
            inner.refresh_worktrees()?;
        }
        // Se olvidan los que ya no existen: los recién borrados y los que ya faltaban
        // (antes estos quedaban para siempre en el registro, sin feature y sin uso).
        inner.mutate(|p| {
            for id in &owned {
                if p.worktree(*id).is_ok_and(|w| w.is_missing()) {
                    p.forget_worktree(*id)?;
                }
            }
            Ok(())
        })?;
        inner.ensure_watches();
        inner.publish(DomainEvent::FeatureRemoved(feature));
        inner.publish(DomainEvent::WorktreesChanged);
        Ok(())
    }

    pub fn attach_worktree(&self, feature: FeatureId, worktree: WorktreeId) -> AppResult<()> {
        self.inner
            .mutate(|p| p.attach_worktree(feature, worktree))?;
        self.inner.publish(DomainEvent::FeatureChanged(feature));
        Ok(())
    }

    pub fn detach_worktree(&self, feature: FeatureId, worktree: WorktreeId) -> AppResult<()> {
        self.inner
            .mutate(|p| p.detach_worktree(feature, worktree))?;
        self.inner.publish(DomainEvent::FeatureChanged(feature));
        Ok(())
    }

    // ── Agentes ──────────────────────────────────────────────────────────────

    pub fn spawn_agent(
        &self,
        feature: FeatureId,
        worktree: WorktreeId,
        provider: ProviderId,
    ) -> AppResult<AgentId> {
        self.inner.config.provider(provider)?;
        let agent = self
            .inner
            .mutate(|p| p.add_agent(feature, worktree, provider))?;
        self.inner.publish(DomainEvent::FeatureChanged(feature));
        self.inner.launch(agent)?;
        Ok(agent)
    }

    pub fn restart_agent(&self, agent: AgentId) -> AppResult<()> {
        self.inner
            .mutate(|p| p.set_agent_status(agent, AgentStatus::Starting))?;
        // Sin este evento la UI seguía mostrando "terminado" hasta el siguiente cambio.
        self.inner.publish(DomainEvent::AgentStatusChanged {
            agent,
            status: AgentStatus::Starting,
        });
        self.inner.launch(agent)
    }

    pub fn stop_agent(&self, agent: AgentId) -> AppResult<()> {
        let active = lock(&self.inner.project).agent(agent)?.status().is_active();
        if active {
            self.inner.ports.agents.kill(agent)?;
        }
        Ok(())
    }

    pub fn remove_agent(&self, agent: AgentId) -> AppResult<()> {
        let removed = self.inner.mutate(|p| p.remove_agent(agent))?;
        self.inner
            .publish(DomainEvent::FeatureChanged(removed.feature()));
        Ok(())
    }

    pub fn rename_agent(&self, agent: AgentId, name: &str) -> AppResult<()> {
        let name = AgentName::parse(name)?;
        self.inner.mutate(|p| p.rename_agent(agent, name))?;
        let feature = lock(&self.inner.project).agent(agent)?.feature();
        self.inner.publish(DomainEvent::FeatureChanged(feature));
        Ok(())
    }

    pub fn agent_input(&self, agent: AgentId, bytes: &[u8]) -> AppResult<()> {
        if let Some(a) = lock(&self.inner.activity).get_mut(&agent) {
            a.on_input(bytes, Instant::now());
        }
        Ok(self.inner.ports.agents.write(agent, bytes)?)
    }

    pub fn agent_resize(&self, agent: AgentId, size: TermSize) -> AppResult<()> {
        Ok(self.inner.ports.agents.resize(agent, size)?)
    }

    /// Conecta una vista a la salida del agente. Si su proceso no existe en esta
    /// sesión (p. ej. tras reabrir la app) se entrega una salida vacía terminada.
    pub fn attach_agent(
        &self,
        agent: AgentId,
        sink: Arc<dyn ChunkSink>,
    ) -> AppResult<Subscription> {
        lock(&self.inner.project).agent(agent)?;
        match self.inner.ports.agents.attach(agent, sink.clone()) {
            Err(e) if e.kind == PortErrorKind::NotFound => {
                sink.send(AgentChunk::Scrollback(Vec::new()));
                sink.send(AgentChunk::Exited(AgentExit::Code(None)));
                Ok(Subscription::noop())
            }
            other => Ok(other?),
        }
    }

    // ── Archivos ─────────────────────────────────────────────────────────────

    pub fn list_dir(&self, worktree: WorktreeId, dir: &str) -> AppResult<Vec<DirEntry>> {
        let root = self.inner.worktree_path(worktree)?;
        Ok(self.inner.ports.fs.list_dir(&root, &RelPath::parse(dir)?)?)
    }

    pub fn read_file(&self, worktree: WorktreeId, file: &str) -> AppResult<FileContent> {
        let root = self.inner.worktree_path(worktree)?;
        Ok(self
            .inner
            .ports
            .fs
            .read_file(&root, &RelPath::parse(file)?)?)
    }

    pub fn write_file(&self, worktree: WorktreeId, file: &str, content: &[u8]) -> AppResult<()> {
        let root = self.inner.worktree_path(worktree)?;
        let rel = RelPath::parse(file)?;
        // `.git` (carpeta o archivo de un worktree enlazado) es de git: escribir ahí
        // podría instalar hooks o corromper el repo. El explorador ya lo oculta.
        if let Some(first) = rel
            .components()
            .next()
            .filter(|c| c.eq_ignore_ascii_case(".git"))
        {
            return Err(PathError::InvalidComponent(first.to_owned()).into());
        }
        Ok(self.inner.ports.fs.write_file(&root, &rel, content)?)
    }

    /// Cambia el nombre (último componente) de un archivo o directorio. Devuelve la ruta nueva.
    pub fn rename_path(
        &self,
        worktree: WorktreeId,
        path: &str,
        new_name: &str,
    ) -> AppResult<RelPath> {
        let root = self.inner.worktree_path(worktree)?;
        let from = RelPath::parse(path)?;
        let name = single_component(new_name)?;
        let to = from.parent().unwrap_or_default().join(&name)?;
        self.inner.ports.fs.rename(&root, &from, &to)?;
        Ok(to)
    }

    /// Pega una copia de `from` dentro del directorio `to_dir` (puede ser otro worktree).
    /// Si el nombre ya existe allí, usa `nombre copia.ext`, `nombre copia 2.ext`…
    /// Devuelve la ruta creada.
    pub fn copy_path(
        &self,
        from_worktree: WorktreeId,
        from: &str,
        to_worktree: WorktreeId,
        to_dir: &str,
    ) -> AppResult<RelPath> {
        let from_root = self.inner.worktree_path(from_worktree)?;
        let to_root = self.inner.worktree_path(to_worktree)?;
        let from = RelPath::parse(from)?;
        let to_dir = RelPath::parse(to_dir)?;
        let Some(name) = from.file_name() else {
            return Err(PathError::InvalidComponent(String::new()).into());
        };
        let fs = &self.inner.ports.fs;
        let taken: Vec<String> = fs
            .list_dir(&to_root, &to_dir)?
            .into_iter()
            .map(|e| e.name)
            .collect();
        let target = to_dir.join(&copy_name(name, |n| {
            taken.iter().any(|t| t.eq_ignore_ascii_case(n))
        }))?;
        fs.copy(&from_root, &from, &to_root, &target)?;
        Ok(target)
    }

    /// Copia archivos o carpetas de fuera de Gryphus (arrastrados desde el sistema o
    /// pegados del portapapeles) dentro de `to_dir`, con la misma regla de nombres que
    /// [`Self::copy_path`]. `sources` son rutas absolutas. Devuelve las rutas creadas;
    /// se detiene en el primer error (lo ya copiado se queda).
    pub fn import_paths(
        &self,
        to_worktree: WorktreeId,
        sources: &[PathBuf],
        to_dir: &str,
    ) -> AppResult<Vec<RelPath>> {
        let to_root = self.inner.worktree_path(to_worktree)?;
        let to_dir = RelPath::parse(to_dir)?;
        let fs = &self.inner.ports.fs;
        let mut taken: Vec<String> = fs
            .list_dir(&to_root, &to_dir)?
            .into_iter()
            .map(|e| e.name)
            .collect();
        let mut created = Vec::with_capacity(sources.len());
        for source in sources {
            let invalid = || PathError::InvalidComponent(source.display().to_string());
            if !source.is_absolute() {
                return Err(invalid().into());
            }
            let (Some(parent), Some(name)) = (source.parent(), source.file_name()) else {
                return Err(invalid().into());
            };
            let name = name.to_str().ok_or_else(invalid)?;
            let from = RelPath::parse(name)?;
            let new_name = copy_name(name, |n| taken.iter().any(|t| t.eq_ignore_ascii_case(n)));
            let target = to_dir.join(&new_name)?;
            fs.copy(parent, &from, &to_root, &target)?;
            taken.push(new_name);
            created.push(target);
        }
        Ok(created)
    }

    /// Ruta absoluta de `path` dentro del worktree (para el portapapeles del sistema).
    pub fn absolute_path(&self, worktree: WorktreeId, path: &str) -> AppResult<PathBuf> {
        let root = self.inner.worktree_path(worktree)?;
        Ok(RelPath::parse(path)?.to_path(&root))
    }

    // ── Git ──────────────────────────────────────────────────────────────────

    pub fn git_status(&self, worktree: WorktreeId) -> AppResult<Vec<FileStatus>> {
        let path = self.inner.worktree_path(worktree)?;
        Ok(self.inner.ports.git.status(&path)?)
    }

    pub fn git_diff(&self, worktree: WorktreeId, base: &DiffBase) -> AppResult<Diff> {
        let path = self.inner.worktree_path(worktree)?;
        Ok(self.inner.ports.git.diff(&path, base)?)
    }

    pub fn git_overview(&self) -> AppResult<GitOverview> {
        let info = self.inner.ports.git.repo_info(&self.inner.root)?;
        let branches = self.inner.ports.git.list_branches(&self.inner.root)?;
        Ok(GitOverview {
            head: info.head,
            branches,
        })
    }

    /// Reconcilia con `git worktree list`. Devuelve si algo cambió.
    pub fn refresh_worktrees(&self) -> AppResult<bool> {
        self.inner.refresh_worktrees()
    }

    // ── Ciclo de vida ────────────────────────────────────────────────────────

    /// Detiene los agentes activos, deja de vigilar y guarda (ADR-013).
    pub fn close(&self) -> AppResult<()> {
        let active: Vec<AgentId> = lock(&self.inner.project)
            .agents()
            .iter()
            .filter(|a| a.status().is_active())
            .map(|a| a.id())
            .collect();
        for agent in active {
            let _ = self.inner.ports.agents.kill(agent);
        }
        lock(&self.inner.watches).clear();
        self.inner.mutate(|_| Ok(()))
    }
}

/// Valida un nombre de archivo nuevo: un solo componente de ruta.
fn single_component(raw: &str) -> AppResult<String> {
    let trimmed = raw.trim();
    let parsed = RelPath::parse(trimmed)?;
    if parsed.is_root() || parsed.as_str() != trimmed || trimmed.contains(['/', '\\']) {
        return Err(PathError::InvalidComponent(raw.to_owned()).into());
    }
    Ok(trimmed.to_owned())
}

/// Nombre libre para una copia: `a.txt` → `a.txt` si está libre; si no
/// `a copia.txt`, `a copia 2.txt`… (los directorios y `.env` no tienen extensión).
pub(crate) fn copy_name(name: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(name) {
        return name.to_owned();
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => name.split_at(i),
        _ => (name, ""),
    };
    (1..)
        .map(|n| {
            if n == 1 {
                format!("{stem} copia{ext}")
            } else {
                format!("{stem} copia {n}{ext}")
            }
        })
        .find(|candidate| !taken(candidate))
        .expect("siempre hay un nombre libre")
}

impl Inner {
    pub(crate) fn publish(&self, event: DomainEvent) {
        self.ports.events.publish(event);
    }

    pub(crate) fn publish_git_changed_for_all(&self) {
        let ids: Vec<WorktreeId> = lock(&self.project)
            .worktrees()
            .iter()
            .filter(|w| !w.is_missing())
            .map(|w| w.id())
            .collect();
        for worktree in ids {
            self.publish(DomainEvent::GitChanged { worktree });
        }
    }

    /// Aplica una mutación de dominio y persiste. El lock se suelta antes de guardar.
    fn mutate<T>(&self, f: impl FnOnce(&mut Project) -> Result<T, DomainError>) -> AppResult<T> {
        let _save = lock(&self.save);
        let (value, snapshot) = {
            let mut p = lock(&self.project);
            let value = f(&mut p)?;
            (value, p.clone())
        };
        self.ports.store.save_project(&snapshot)?;
        Ok(value)
    }

    fn worktree_path(&self, worktree: WorktreeId) -> AppResult<PathBuf> {
        let p = lock(&self.project);
        let wt = p.worktree(worktree)?;
        if wt.is_missing() {
            return Err(DomainError::WorktreeMissing(worktree).into());
        }
        Ok(wt.path().to_path_buf())
    }

    pub(crate) fn refresh_worktrees(self: &Arc<Self>) -> AppResult<bool> {
        let infos = self.ports.git.list_worktrees(&self.root)?;
        let changed = {
            let mut p = lock(&self.project);
            p.sync_worktrees(&infos)
        };
        if changed {
            self.mutate(|_| Ok(()))?;
            self.ensure_watches();
            self.publish(DomainEvent::WorktreesChanged);
        }
        Ok(changed)
    }

    /// Vigila cada worktree presente y deja de vigilar los que ya no están.
    fn ensure_watches(self: &Arc<Self>) {
        let present: Vec<(WorktreeId, PathBuf, bool)> = lock(&self.project)
            .worktrees()
            .iter()
            .filter(|w| !w.is_missing())
            .map(|w| (w.id(), w.path().to_path_buf(), w.is_main()))
            .collect();
        let missing: Vec<(WorktreeId, PathBuf, bool)> = {
            let mut watches = lock(&self.watches);
            watches.retain(|id, _| present.iter().any(|(p, _, _)| p == id));
            present
                .into_iter()
                .filter(|(id, _, _)| !watches.contains_key(id))
                .collect()
        };
        for (id, path, is_main) in missing {
            let sink = Arc::new(WatchSink {
                session: Arc::downgrade(self),
                worktree: id,
                is_main,
            });
            // Un worktree que no se puede vigilar sigue siendo usable.
            if let Ok(sub) = self.ports.fs.watch(&path, sink) {
                lock(&self.watches).insert(id, sub);
            }
        }
    }

    fn rollback_worktree(self: &Arc<Self>, path: &Path) {
        let _ = self.ports.git.remove_worktree(&self.root, path, true);
        let _ = self.refresh_worktrees();
        let missing: Vec<WorktreeId> = lock(&self.project)
            .worktrees()
            .iter()
            .filter(|w| w.is_missing() && w.path() == path)
            .map(|w| w.id())
            .collect();
        for id in missing {
            let _ = self.mutate(|p| p.forget_worktree(id).map(|_| ()));
        }
    }

    /// Lanza el proceso de un agente en estado `Starting`.
    fn launch(self: &Arc<Self>, agent: AgentId) -> AppResult<()> {
        let (provider, cwd) = {
            let p = lock(&self.project);
            let a = p.agent(agent)?;
            (a.provider(), p.worktree(a.worktree())?.path().to_path_buf())
        };
        let config = self.config.provider(provider)?;
        let spec = SpawnSpec::new(agent, config, cwd, TermSize::default());
        let weak = Arc::downgrade(self);
        let on_exit = Box::new(move |exit: AgentExit| {
            if let Some(inner) = weak.upgrade() {
                inner.set_status(agent, exit.into());
            }
        });
        match self.ports.agents.spawn(spec, on_exit) {
            Ok(()) => {
                self.monitor(agent);
                // Si el proceso ya terminó (salida instantánea), se respeta ese estado.
                let still_starting = lock(&self.project)
                    .agent(agent)
                    .is_ok_and(|a| *a.status() == AgentStatus::Starting);
                if still_starting {
                    self.set_status(agent, AgentStatus::Running);
                }
                Ok(())
            }
            Err(e) => {
                self.set_status(
                    agent,
                    AgentStatus::Failed {
                        reason: e.message.clone(),
                    },
                );
                Err(e.into())
            }
        }
    }

    fn set_status(&self, agent: AgentId, status: AgentStatus) {
        if !status.is_active() {
            self.unmonitor(agent);
        }
        if self
            .mutate(|p| p.set_agent_status(agent, status.clone()))
            .is_ok()
        {
            self.publish(DomainEvent::AgentStatusChanged { agent, status });
        }
    }

    /// Empieza a vigilar la salida del agente para avisar cuando termine su tarea.
    fn monitor(self: &Arc<Self>, agent: AgentId) {
        lock(&self.activity).insert(agent, Activity::new(Instant::now()));
        let sink = Arc::new(ActivitySink {
            session: Arc::downgrade(self),
            agent,
        });
        match self.ports.agents.attach(agent, sink) {
            // Sin la vigilancia el agente sigue siendo usable; solo no habrá aviso.
            Ok(sub) => {
                let old = lock(&self.monitors).insert(agent, sub);
                drop(old);
            }
            Err(_) => {
                lock(&self.activity).remove(&agent);
            }
        }
    }

    fn unmonitor(&self, agent: AgentId) {
        lock(&self.activity).remove(&agent);
        // Se suelta fuera del lock: cancelar toma el lock de salida del runtime.
        let sub = lock(&self.monitors).remove(&agent);
        drop(sub);
    }

    /// Salida en vivo de un agente (desde [`ActivitySink`]).
    pub(crate) fn agent_output(&self, agent: AgentId, bytes: &[u8]) {
        let notified = lock(&self.activity)
            .get_mut(&agent)
            .is_some_and(|a| a.on_output(bytes, Instant::now()));
        if notified {
            self.publish(DomainEvent::AgentIdle { agent });
        }
    }

    /// Agentes que acaban de quedar en silencio tras trabajar.
    fn poll_idle(&self) -> Vec<AgentId> {
        let now = Instant::now();
        lock(&self.activity)
            .iter_mut()
            .filter_map(|(id, a)| a.poll(now).then_some(*id))
            .collect()
    }
}

/// Revisa periódicamente el silencio de los agentes. Termina al cerrarse la sesión.
fn spawn_idle_ticker(session: Weak<Inner>) {
    let _ = std::thread::Builder::new()
        .name("gryphus-agent-idle".into())
        .spawn(move || loop {
            std::thread::sleep(POLL_EVERY);
            let Some(inner) = session.upgrade() else {
                return;
            };
            for agent in inner.poll_idle() {
                inner.publish(DomainEvent::AgentIdle { agent });
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_names_avoid_collisions() {
        let taken = ["a.txt", "a copia.txt", "dir", ".env"];
        let is_taken = |n: &str| taken.contains(&n);
        assert_eq!(copy_name("b.txt", is_taken), "b.txt");
        assert_eq!(copy_name("a.txt", is_taken), "a copia 2.txt");
        assert_eq!(copy_name("dir", is_taken), "dir copia");
        assert_eq!(copy_name(".env", is_taken), ".env copia");
    }

    #[test]
    fn new_names_are_a_single_component() {
        assert_eq!(single_component("  b.txt ").unwrap(), "b.txt");
        for bad in ["", "a/b", "..", "a\\b", "c:x", "."] {
            assert!(single_component(bad).is_err(), "{bad:?}");
        }
    }
}
