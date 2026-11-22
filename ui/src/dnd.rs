//! Archivos arrastrados desde fuera de la app (explorador del sistema, escritorio…).
//!
//! WebView2 no entrega a la página las rutas de los archivos soltados: Tauri los
//! intercepta y emite `tauri://drag-*` con las rutas absolutas y la posición (en
//! píxeles físicos). Aquí se decide qué hay debajo y qué hacer:
//! - Terminal del agente: se escriben las rutas (entre comillas si hace falta), como
//!   en VS Code; los agentes las usan como adjuntos (p. ej. imágenes).
//! - Explorador: se copian dentro de la carpeta bajo el puntero (o de la carpeta del
//!   archivo bajo el puntero, o de la raíz).

use leptos::prelude::*;
use serde::de::IgnoredAny;
use serde::Deserialize;

use crate::bridge::dom::{hit_test, Hit};
use crate::bridge::tauri::listen;
use crate::logic;
use crate::store::{FileDrop, Store};

const DRAG_ENTER: &str = "tauri://drag-enter";
const DRAG_OVER: &str = "tauri://drag-over";
const DRAG_DROP: &str = "tauri://drag-drop";
const DRAG_LEAVE: &str = "tauri://drag-leave";

#[derive(Debug, Deserialize)]
struct Position {
    x: f64,
    y: f64,
}

#[derive(Debug, Deserialize)]
struct DragPayload {
    #[serde(default)]
    paths: Vec<String>,
    position: Position,
}

/// Destino bajo el punto, si acepta archivos.
fn target_at(store: Store, pos: &Position) -> Option<FileDrop> {
    match hit_test(pos.x, pos.y)? {
        Hit::Terminal => store
            .selected_agent
            .get_untracked()
            .map(|_| FileDrop::Terminal),
        Hit::Explorer(row) => {
            let worktree = store.active_worktree.get_untracked()?;
            let dir = match row {
                Some((path, true)) => path,
                Some((path, false)) => logic::parent_dir(&path).to_owned(),
                None => String::new(),
            };
            Some(FileDrop::Explorer { worktree, dir })
        }
    }
}

fn set_target(store: Store, target: Option<FileDrop>) {
    if store.drop_target.with_untracked(|t| *t != target) {
        store.drop_target.set(target);
    }
}

pub fn register(store: Store) {
    let hover = move |p: DragPayload| set_target(store, target_at(store, &p.position));
    listen(DRAG_ENTER, hover);
    listen(DRAG_OVER, hover);
    listen(DRAG_LEAVE, move |_: IgnoredAny| set_target(store, None));
    listen(DRAG_DROP, move |p: DragPayload| {
        set_target(store, None);
        if p.paths.is_empty() {
            return;
        }
        match target_at(store, &p.position) {
            Some(FileDrop::Terminal) => {
                store
                    .terminal_paste
                    .set(Some(logic::terminal_paths(&p.paths)));
            }
            Some(FileDrop::Explorer { worktree, dir }) => store.import(worktree, dir, p.paths),
            None => {}
        }
    });
}
