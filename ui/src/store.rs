//! Estado global de la ventana (señales). Es caché de lo que devuelve el backend
//! más estado puro de UI; la fuente de verdad está en Rust.

use std::collections::HashSet;

use futures_channel::oneshot;

use gryphus_ipc::{FileStatusDto, GitOverviewDto, IpcError, ProjectDto};
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::bridge::prefs;
use crate::ipc;
use crate::layout::CenterView;
use crate::logic;
use crate::settings::{Settings, STORAGE_KEY};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OpenFile {
    pub worktree: u64,
    pub path: String,
}

impl OpenFile {
    /// Clave del documento en el editor JS.
    pub fn key(&self) -> String {
        format!("{}:{}", self.worktree, self.path)
    }

    pub fn name(&self) -> &str {
        self.path.rsplit_once('/').map_or(&self.path, |(_, n)| n)
    }
}

/// Qué botones muestra el modal de confirmación.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmKind {
    /// Cancelar / Aceptar.
    Accept,
    /// Cancelar / No guardar / Guardar.
    SaveChanges,
}

/// Respuesta del modal. En [`ConfirmKind::Accept`] solo hay `Accept` o `Cancel`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Accept,
    Discard,
    Cancel,
}

/// Confirmación pendiente (modal `app::ConfirmDialog`).
pub struct ConfirmRequest {
    pub message: String,
    pub kind: ConfirmKind,
    tx: oneshot::Sender<Answer>,
}

#[derive(Clone, Copy)]
pub struct Store {
    pub confirm: RwSignal<Option<ConfirmRequest>>,
    pub loaded: RwSignal<bool>,
    pub project: RwSignal<Option<ProjectDto>>,
    pub selected_feature: RwSignal<Option<u64>>,
    pub selected_agent: RwSignal<Option<u64>>,
    /// Worktree que siguen el explorador, el diff y la barra de estado.
    pub active_worktree: RwSignal<Option<u64>>,
    pub view: RwSignal<CenterView>,
    /// Pestaña activa del editor. Asignarla abre el archivo en una pestaña si no lo estaba.
    pub open_file: RwSignal<Option<OpenFile>>,
    /// Pestañas abiertas del editor, en orden.
    pub open_files: RwSignal<Vec<OpenFile>>,
    /// Pestañas con cambios sin guardar.
    pub dirty_files: RwSignal<HashSet<OpenFile>>,
    pub status: RwSignal<Vec<FileStatusDto>>,
    pub overview: RwSignal<Option<GitOverviewDto>>,
    /// Último lote de cambios en disco: (worktree, rutas).
    pub fs_change: RwSignal<(u64, Vec<String>)>,
    pub fs_tick: RwSignal<u64>,
    pub git_tick: RwSignal<u64>,
    pub error: RwSignal<Option<String>>,
    /// Directorios expandidos en el explorador: (worktree, ruta). Vive aquí y no en
    /// cada fila para sobrevivir a las recargas por cambios en disco.
    pub expanded: RwSignal<HashSet<(u64, String)>>,
    /// Las tablas de iconos ya se cargaron (el explorador se repinta al cambiar).
    pub icons_ready: RwSignal<bool>,
    pub settings: RwSignal<Settings>,
    pub settings_open: RwSignal<bool>,
}

impl Store {
    pub fn new() -> Self {
        Self {
            confirm: RwSignal::new(None),
            loaded: RwSignal::new(false),
            project: RwSignal::new(None),
            selected_feature: RwSignal::new(None),
            selected_agent: RwSignal::new(None),
            active_worktree: RwSignal::new(None),
            view: RwSignal::new(CenterView::Agent),
            open_file: RwSignal::new(None),
            open_files: RwSignal::new(Vec::new()),
            dirty_files: RwSignal::new(HashSet::new()),
            status: RwSignal::new(Vec::new()),
            overview: RwSignal::new(None),
            fs_change: RwSignal::new((0, Vec::new())),
            fs_tick: RwSignal::new(0),
            git_tick: RwSignal::new(0),
            error: RwSignal::new(None),
            expanded: RwSignal::new(HashSet::new()),
            icons_ready: RwSignal::new(false),
            settings: RwSignal::new(Settings::default()),
            settings_open: RwSignal::new(false),
        }
    }

    /// Pide confirmación con el modal de la app. Una petición nueva cancela la anterior.
    pub async fn ask(self, message: impl Into<String>) -> bool {
        self.prompt(message.into(), ConfirmKind::Accept).await == Answer::Accept
    }

    /// Pregunta si guardar los cambios: Guardar (`Accept`), No guardar (`Discard`) o `Cancel`.
    pub async fn ask_save(self, message: impl Into<String>) -> Answer {
        self.prompt(message.into(), ConfirmKind::SaveChanges).await
    }

    async fn prompt(self, message: String, kind: ConfirmKind) -> Answer {
        if let Some(accepted) = crate::bridge::dialog::e2e_auto_confirm(&message) {
            return if accepted { Answer::Accept } else { Answer::Cancel };
        }
        let (tx, rx) = oneshot::channel();
        self.confirm.set(Some(ConfirmRequest { message, kind, tx }));
        rx.await.unwrap_or(Answer::Cancel)
    }

    /// Responde la confirmación pendiente y cierra el modal.
    pub fn answer(&self, answer: Answer) {
        if let Some(req) = self.confirm.try_update(Option::take).flatten() {
            let _ = req.tx.send(answer);
        }
    }

    /// Carga las preferencias guardadas y las aplica.
    pub fn load_settings(&self) {
        let s = prefs::load(STORAGE_KEY)
            .map(|t| Settings::parse(&t))
            .unwrap_or_default();
        prefs::apply_css_vars(s.css_vars());
        self.settings.set(s);
    }

    /// Aplica y guarda nuevas preferencias.
    pub fn set_settings(&self, s: Settings) {
        prefs::apply_css_vars(s.css_vars());
        prefs::save(STORAGE_KEY, &s.serialize());
        self.settings.set(s);
    }

    /// Abre un repositorio: en esta ventana si está vacía; si no, en otra (un proyecto por ventana).
    pub fn open_project(self, path: String) {
        if path.trim().is_empty() {
            return;
        }
        spawn_local(async move {
            match ipc::open_project(path).await {
                Ok(res) if !res.other_window => self.refresh_project(),
                Ok(_) => {}
                Err(e) => self.report(e),
            }
        });
    }

    /// Diálogo nativo de carpeta y luego [`Self::open_project`].
    pub fn pick_and_open(self) {
        spawn_local(async move {
            match ipc::pick_folder().await {
                Ok(Some(folder)) => self.open_project(folder),
                Ok(None) => {}
                Err(e) => self.report(e),
            }
        });
    }

    /// Cierra el proyecto de la ventana y vuelve a la bienvenida.
    pub fn close_project(self) {
        spawn_local(async move {
            if let Err(e) = ipc::close_project().await {
                self.report(e);
            }
            self.selected_agent.set(None);
            self.selected_feature.set(None);
            self.active_worktree.set(None);
            self.open_file.set(None);
            self.open_files.set(Vec::new());
            self.dirty_files.set(HashSet::new());
            self.status.set(Vec::new());
            self.overview.set(None);
            self.expanded.set(HashSet::new());
            self.view.set(CenterView::Agent);
            self.refresh_project();
        });
    }

    pub fn is_expanded(&self, worktree: u64, path: &str) -> bool {
        self.expanded
            .with(|set| set.contains(&(worktree, path.to_owned())))
    }

    pub fn toggle_expanded(&self, worktree: u64, path: &str) {
        self.expanded.update(|set| {
            let key = (worktree, path.to_owned());
            if !set.remove(&key) {
                set.insert(key);
            }
        });
    }

    pub fn report(&self, e: IpcError) {
        self.error.set(Some(format!("{} — {}", e.message, e.code)));
    }

    pub fn bump_git(&self) {
        self.git_tick.update(|t| *t += 1);
    }

    /// Vuelve a pedir el proyecto y corrige la selección si algo desapareció.
    pub fn refresh_project(self) {
        spawn_local(async move {
            match ipc::get_project().await {
                Ok(project) => {
                    self.project.set(project);
                    self.loaded.set(true);
                    self.fix_selection();
                    self.bump_git();
                }
                Err(e) => {
                    self.loaded.set(true);
                    self.report(e);
                }
            }
        });
    }

    fn fix_selection(&self) {
        self.project.with_untracked(|p| {
            let Some(p) = p else { return };
            let active_ok = self
                .active_worktree
                .get_untracked()
                .and_then(|id| logic::worktree(p, id))
                .is_some_and(|w| !w.missing);
            if !active_ok {
                self.active_worktree
                    .set(logic::main_worktree(p).map(|w| w.id));
            }
            if let Some(agent) = self.selected_agent.get_untracked() {
                if !p.agents.iter().any(|a| a.id == agent) {
                    self.selected_agent.set(None);
                }
            }
            if let Some(f) = self.selected_feature.get_untracked() {
                if logic::feature(p, f).is_none() {
                    self.selected_feature.set(None);
                }
            }
        });
    }

    pub fn select_agent(&self, agent: u64, worktree: u64, feature: u64) {
        self.selected_agent.set(Some(agent));
        self.selected_feature.set(Some(feature));
        self.active_worktree.set(Some(worktree));
        self.view.set(CenterView::Agent);
    }
}
