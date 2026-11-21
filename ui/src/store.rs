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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenFile {
    pub worktree: u64,
    pub path: String,
}

/// Confirmación pendiente (modal `app::ConfirmDialog`): "Cancelar" más una o
/// varias opciones; la primera es la principal (Enter).
pub struct ConfirmRequest {
    pub message: String,
    pub choices: Vec<String>,
    tx: oneshot::Sender<Option<usize>>,
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
    pub open_file: RwSignal<Option<OpenFile>>,
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
    /// El editor tiene cambios sin guardar (para avisar al cerrar proyecto o ventana).
    pub editor_dirty: RwSignal<bool>,
    /// Número de la última petición de `get_project`: una respuesta más antigua que
    /// llega tarde no debe pisar el estado nuevo.
    project_req: StoredValue<u64>,
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
            editor_dirty: RwSignal::new(false),
            project_req: StoredValue::new(0),
        }
    }

    /// Pide confirmación con el modal de la app. Una petición nueva cancela la anterior.
    pub async fn ask(self, message: impl Into<String>) -> bool {
        self.choose(message, ["Aceptar"]).await == Some(0)
    }

    /// Pide elegir entre varias opciones. `None` = cancelado (botón, Esc o fondo).
    pub async fn choose<const N: usize>(
        self,
        message: impl Into<String>,
        choices: [&str; N],
    ) -> Option<usize> {
        let message = message.into();
        if let Some(answer) = crate::bridge::dialog::e2e_auto_confirm(&message) {
            return answer.then_some(0);
        }
        let (tx, rx) = oneshot::channel();
        self.confirm.set(Some(ConfirmRequest {
            message,
            choices: choices.iter().map(|c| (*c).to_owned()).collect(),
            tx,
        }));
        rx.await.ok().flatten()
    }

    /// Responde la confirmación pendiente y cierra el modal.
    pub fn answer(&self, choice: Option<usize>) {
        if let Some(req) = self.confirm.try_update(Option::take).flatten() {
            let _ = req.tx.send(choice);
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
            self.status.set(Vec::new());
            self.overview.set(None);
            self.expanded.set(HashSet::new());
            self.view.set(CenterView::Agent);
            self.refresh_project();
        });
    }

    /// Qué se perdería al cerrar (agentes activos, cambios sin guardar), o `None`.
    pub fn close_warning(&self) -> Option<String> {
        let active = self.project.with_untracked(|p| {
            p.as_ref().map_or(0, |p| {
                p.agents.iter().filter(|a| a.status.is_active()).count()
            })
        });
        let mut parts = Vec::new();
        if active > 0 {
            parts.push(format!("se detendrán {active} agente(s) en ejecución"));
        }
        if self.editor_dirty.get_untracked() {
            parts.push("se perderán los cambios sin guardar del editor".to_owned());
        }
        (!parts.is_empty()).then(|| format!("Al cerrar, {}. ¿Continuar?", parts.join(" y ")))
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
        self.project_req.update_value(|n| *n += 1);
        let req = self.project_req.get_value();
        spawn_local(async move {
            let res = ipc::get_project().await;
            if self.project_req.get_value() != req {
                return; // ya hay una petición más reciente en curso
            }
            match res {
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
