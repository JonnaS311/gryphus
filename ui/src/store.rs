//! Estado global de la ventana (señales). Es caché de lo que devuelve el backend
//! más estado puro de UI; la fuente de verdad está en Rust.

use std::collections::HashSet;

use gryphus_ipc::{FileStatusDto, GitOverviewDto, IpcError, ProjectDto};
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::ipc;
use crate::layout::CenterView;
use crate::logic;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenFile {
    pub worktree: u64,
    pub path: String,
}

#[derive(Clone, Copy)]
pub struct Store {
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
}

impl Store {
    pub fn new() -> Self {
        Self {
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
        }
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
