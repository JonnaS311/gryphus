//! Archivos arrastrados desde fuera de la app (explorador del sistema, escritorio…)
//! y filas arrastradas desde el explorador de Gryphus ([`entry_target`]; el arrastre
//! interno usa eventos del puntero, ver `panels::explorer`).
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

use crate::bridge::dom::{hit_test, hit_test_css, Hit};
use crate::bridge::tauri::listen;
use crate::logic;
use crate::store::{EntryDrag, FileDrop, Store};

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

/// Carpeta del explorador bajo el puntero: la fila si es carpeta, la de su archivo
/// si no, o la raíz.
fn explorer_dir(store: Store, row: Option<(String, bool)>) -> Option<FileDrop> {
    let worktree = store.active_worktree.get_untracked()?;
    let dir = match row {
        Some((path, true)) => path,
        Some((path, false)) => logic::parent_dir(&path).to_owned(),
        None => String::new(),
    };
    Some(FileDrop::Explorer { worktree, dir })
}

/// Destino bajo el punto, si acepta archivos.
fn target_at(store: Store, pos: &Position) -> Option<FileDrop> {
    match hit_test(pos.x, pos.y)? {
        Hit::Terminal => store
            .selected_agent
            .get_untracked()
            .map(|_| FileDrop::Terminal),
        // El editor solo abre archivos del worktree (arrastrados desde el explorador).
        Hit::Editor => None,
        Hit::Explorer(row) => explorer_dir(store, row),
    }
}

/// Destino de una fila del explorador arrastrada hasta (`x`, `y`) en píxeles CSS:
/// terminal (escribe la ruta), editor (abre el archivo) u otra carpeta (la mueve).
pub fn entry_target(store: Store, drag: &EntryDrag, x: f64, y: f64) -> Option<FileDrop> {
    match hit_test_css(x, y)? {
        Hit::Terminal => store
            .selected_agent
            .get_untracked()
            .map(|_| FileDrop::Terminal),
        Hit::Editor => (!drag.is_dir).then_some(FileDrop::Editor),
        Hit::Explorer(row) => explorer_dir(store, row).filter(|t| {
            matches!(t, FileDrop::Explorer { worktree, dir }
                if *worktree == drag.worktree && logic::can_move_into(&drag.path, dir))
        }),
    }
}

pub fn set_target(store: Store, target: Option<FileDrop>) {
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
            Some(FileDrop::Editor) | None => {}
        }
    });
}
