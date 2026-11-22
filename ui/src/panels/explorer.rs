//! Panel derecho: árbol perezoso del worktree activo (un nivel por expansión).

use gryphus_ipc::{DirEntryDto, EntryKindDto, FileStateDto};
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::bridge::icons as bridge_icons;
use crate::icons;
use crate::ipc;
use crate::layout::CenterView;
use crate::logic;
use crate::panels::InlineRename;
use crate::store::{EntryDrag, FileDrop, MenuTarget, OpenFile, RenameTarget, Store};

/// Píxeles que hay que mover el puntero para que un clic en una fila pase a arrastre.
const DRAG_THRESHOLD: i32 = 6;

/// Arrastre de filas del explorador hacia la terminal, el editor u otra carpeta.
/// Va con eventos del puntero y no con HTML5 drag & drop: con el soltar de archivos
/// de Tauri activo, WebView2 no entrega los `drop` internos.
#[derive(Clone, Copy)]
struct RowDrag {
    /// Fila pulsada y dónde, mientras no se suelte el botón.
    press: StoredValue<Option<(EntryDrag, i32, i32)>>,
    /// Posición del puntero (para la etiqueta que lo sigue).
    pos: RwSignal<(i32, i32)>,
    /// El último `pointerup` terminó un arrastre: el `click` que le sigue no cuenta.
    swallow_click: StoredValue<bool>,
}

impl RowDrag {
    fn start(&self, ev: &leptos::ev::PointerEvent, drag: EntryDrag) {
        self.swallow_click.set_value(false);
        if ev.button() == 0 {
            self.press
                .set_value(Some((drag, ev.client_x(), ev.client_y())));
        }
    }

    /// `true` si el clic viene de soltar un arrastre (y lo consume).
    fn take_click(&self) -> bool {
        let swallow = self.swallow_click.get_value();
        self.swallow_click.set_value(false);
        swallow
    }
}

#[component]
pub fn Explorer() -> impl IntoView {
    let store = expect_context::<Store>();
    let row_drag = RowDrag {
        press: StoredValue::new(None),
        pos: RwSignal::new((0, 0)),
        swallow_click: StoredValue::new(false),
    };
    provide_context(row_drag);
    let reset = move || {
        row_drag.press.set_value(None);
        if store.entry_drag.get_untracked().is_some() {
            store.entry_drag.set(None);
            crate::dnd::set_target(store, None);
        }
    };
    let on_move = window_event_listener(leptos::ev::pointermove, move |ev| {
        let Some((drag, x, y)) = row_drag.press.try_get_value().flatten() else {
            return;
        };
        // Se soltó el botón fuera de la ventana: no llegó `pointerup`.
        if ev.buttons() & 1 == 0 {
            reset();
            return;
        }
        let (cx, cy) = (ev.client_x(), ev.client_y());
        if store.entry_drag.get_untracked().is_none() {
            if (cx - x).abs() <= DRAG_THRESHOLD && (cy - y).abs() <= DRAG_THRESHOLD {
                return;
            }
            store.menu.set(None);
            store.entry_drag.set(Some(drag.clone()));
        }
        row_drag.pos.set((cx, cy));
        let target = crate::dnd::entry_target(store, &drag, f64::from(cx), f64::from(cy));
        crate::dnd::set_target(store, target);
    });
    let on_up = window_event_listener(leptos::ev::pointerup, move |_| {
        if row_drag.press.try_get_value().flatten().is_none() {
            return;
        }
        if let Some(drag) = store.entry_drag.get_untracked() {
            row_drag.swallow_click.set_value(true);
            if let Some(target) = store.drop_target.get_untracked() {
                store.drop_entry(drag, target);
            }
        }
        reset();
    });
    let on_cancel = window_event_listener(leptos::ev::pointercancel, move |_| {
        if row_drag.press.try_get_value().flatten().is_some() {
            reset();
        }
    });
    let on_key = window_event_listener(leptos::ev::keydown, move |ev| {
        if ev.key() == "Escape" && row_drag.press.try_get_value().flatten().is_some() {
            reset();
        }
    });
    on_cleanup(move || {
        on_move.remove();
        on_up.remove();
        on_cancel.remove();
        on_key.remove();
    });
    let ghost = move || {
        store.entry_drag.with(|d| {
            d.as_ref().map(|d| {
                let name = d.path.rsplit('/').next().unwrap_or(&d.path).to_owned();
                let hint = match store.drop_target.get() {
                    Some(FileDrop::Terminal) => "escribir la ruta en la terminal",
                    Some(FileDrop::Editor) => "abrir en el editor",
                    Some(FileDrop::Explorer { .. }) => "mover aquí",
                    None => "",
                };
                let (x, y) = row_drag.pos.get();
                view! {
                    <div
                        class="drag-ghost"
                        style:left=format!("{}px", x + 14)
                        style:top=format!("{}px", y + 10)
                    >
                        <span>{name}</span>
                        {(!hint.is_empty()).then(|| view! { <span class="muted">{hint}</span> })}
                    </div>
                }
            })
        })
    };
    let label = move || {
        store.project.with(|p| {
            p.as_ref()
                .zip(store.active_worktree.get())
                .and_then(|(p, id)| logic::worktree(p, id))
                .map(logic::worktree_label)
                .unwrap_or_default()
        })
    };

    // Ctrl+C / Ctrl+V (⌘ en macOS) con el foco en el explorador (tras pulsar una fila).
    let on_keydown = move |ev: leptos::ev::KeyboardEvent| {
        if !crate::bridge::dom::primary_mod(&ev) || ev.alt_key() || ev.shift_key() {
            return;
        }
        let Some(worktree) = store.active_worktree.get_untracked() else {
            return;
        };
        let selected = store
            .explorer_selected
            .get_untracked()
            .filter(|(wt, _, _)| *wt == worktree);
        match ev.key().to_lowercase().as_str() {
            "c" => {
                if let Some((_, path, _)) = selected {
                    ev.prevent_default();
                    store.copy_entry(worktree, path);
                }
            }
            "v" => {
                ev.prevent_default();
                let dir = match selected {
                    Some((_, path, true)) => path,
                    Some((_, path, false)) => logic::parent_dir(&path).to_owned(),
                    None => String::new(),
                };
                store.paste(worktree, dir);
            }
            _ => {}
        }
    };
    let root_drop = move || {
        store
            .drop_target
            .with(|t| matches!(t, Some(FileDrop::Explorer { dir, .. }) if dir.is_empty()))
    };

    view! {
        <aside
            class="pane pane-right"
            class:drop-target=root_drop
            tabindex="-1"
            on:keydown=on_keydown
            on:contextmenu=move |ev| {
                if let Some(worktree) = store.active_worktree.get_untracked() {
                    store.open_menu(&ev, MenuTarget::ExplorerRoot { worktree });
                }
            }
        >
            <header class="pane-header">
                <h2 class="pane-title">"Explorador"</h2>
                <span class="muted ellipsis" title=label>{label}</span>
            </header>
            {move || match store.active_worktree.get() {
                Some(worktree) => {
                    view! { <TreeDir worktree path=String::new() depth=0/> }.into_any()
                }
                None => view! { <p class="hint">"Sin worktree activo"</p> }.into_any(),
            }}
            {ghost}
        </aside>
    }
}

#[component]
fn TreeDir(worktree: u64, path: String, depth: usize) -> impl IntoView {
    let store = expect_context::<Store>();
    let (entries, set_entries) = signal(Vec::<DirEntryDto>::new());
    let dir = path.clone();
    Effect::new(move |_| {
        store.fs_tick.track();
        let dir = dir.clone();
        spawn_local(async move {
            match ipc::list_dir(worktree, dir).await {
                // Solo se reemplaza si cambió: evita repintar el árbol en cada evento.
                Ok(list) => {
                    if entries.try_with_untracked(|current| *current != list) == Some(true) {
                        let _ = set_entries.try_set(list);
                    }
                }
                Err(e) if e.code == "NOT_FOUND" => {
                    let _ = set_entries.try_set(Vec::new());
                }
                Err(e) => store.report(e),
            }
        });
    });

    view! {
        <ul class="tree">
            {move || {
                entries
                    .get()
                    .into_iter()
                    .map(|entry| view! { <TreeEntry worktree entry parent=path.clone() depth/> })
                    .collect_view()
            }}
        </ul>
    }
}

fn mark_class(state: FileStateDto) -> &'static str {
    match state {
        FileStateDto::Added | FileStateDto::Untracked => "mark added",
        FileStateDto::Deleted => "mark deleted",
        FileStateDto::Conflicted => "mark conflict",
        FileStateDto::Modified | FileStateDto::Renamed => "mark modified",
    }
}

/// Nombre de la fila, o el campo de renombrar si está en edición.
fn entry_name(worktree: u64, path: String, name: String, is_file: bool) -> impl IntoView {
    let store = expect_context::<Store>();
    let target = RenameTarget::Entry {
        worktree,
        path: path.clone(),
    };
    let renaming = {
        let t = target.clone();
        move || store.is_renaming(&t)
    };
    let commit = move |new_name: String| {
        let old = path.clone();
        if let Some(dirty) = store.dirty_under(worktree, &old) {
            store.error.set(Some(format!(
                "Guarda o cierra «{}» antes de renombrar: tiene cambios sin guardar.",
                dirty.name()
            )));
            return;
        }
        spawn_local(async move {
            match ipc::rename_path(worktree, old.clone(), new_name).await {
                Ok(new) => store.path_renamed(worktree, &old, &new),
                Err(e) => store.report(e),
            }
        });
    };
    view! {
        <Show
            when=renaming
            fallback={
                let name = name.clone();
                move || view! { <span class="name">{name.clone()}</span> }
            }
        >
            <InlineRename initial=name.clone() target=target.clone() is_file on_commit=commit.clone()/>
        </Show>
    }
}

#[component]
fn TreeEntry(worktree: u64, entry: DirEntryDto, parent: String, depth: usize) -> impl IntoView {
    let store = expect_context::<Store>();
    let full = logic::join_path(&parent, &entry.name);
    let padding = format!("{}px", 8 + depth * 12);
    let name = entry.name.clone();
    let menu_path = full.clone();
    let is_dir = entry.kind == EntryKindDto::Dir;
    let row_drag = expect_context::<RowDrag>();
    let press_path = full.clone();
    let on_press = move |ev: leptos::ev::PointerEvent| {
        let renaming = RenameTarget::Entry {
            worktree,
            path: press_path.clone(),
        };
        // Arrastrar dentro del campo de renombrar selecciona texto.
        if !store.is_renaming(&renaming) {
            row_drag.start(
                &ev,
                EntryDrag {
                    worktree,
                    path: press_path.clone(),
                    is_dir,
                },
            );
        }
    };
    let lifted_path = full.clone();
    let lifted = move || {
        store.entry_drag.with(|d| {
            d.as_ref()
                .is_some_and(|d| d.worktree == worktree && d.path == lifted_path)
        })
    };
    let on_menu = move |ev: leptos::ev::MouseEvent| {
        store.open_menu(
            &ev,
            MenuTarget::Entry {
                worktree,
                path: menu_path.clone(),
                is_dir,
            },
        )
    };

    match entry.kind {
        EntryKindDto::Dir => {
            let (open_key, chev_key) = (full.clone(), full.clone());
            let open = move || store.is_expanded(worktree, &open_key);
            let chev_open = move || store.is_expanded(worktree, &chev_key);
            let icon_key = full.clone();
            let icon_open = move || store.is_expanded(worktree, &icon_key);
            let toggle_key = full.clone();
            let changed_path = full.clone();
            let changed = move || store.status.with(|s| logic::dir_changed(s, &changed_path));
            let drop_path = full.clone();
            let is_drop = move || {
                store.drop_target.with(|t| {
                    matches!(t, Some(FileDrop::Explorer { worktree: w, dir }) if *w == worktree && *dir == drop_path)
                })
            };
            view! {
                <li>
                    <div
                        class="tree-row dir"
                        class:ignored=entry.ignored
                        class:drop-target=is_drop
                        class:lifted=lifted
                        style:padding-left=padding
                        data-path=full.clone()
                        on:pointerdown=on_press
                        on:click=move |_| {
                            if row_drag.take_click() {
                                return;
                            }
                            store.explorer_selected.set(Some((worktree, toggle_key.clone(), true)));
                            store.toggle_expanded(worktree, &toggle_key)
                        }
                        on:contextmenu=on_menu
                    >
                        <span class="chev">{move || if chev_open() { "▾" } else { "▸" }}</span>
                        <img
                            class="ficon"
                            alt=""
                            src={
                                let name = name.clone();
                                move || {
                                    store.icons_ready.track();
                                    icons::url(&icons::folder_icon(&name, icon_open(), bridge_icons::get))
                                }
                            }
                        />
                        {entry_name(worktree, full.clone(), name, false)}
                        <Show when=changed>
                            <span class="mark modified">"•"</span>
                        </Show>
                    </div>
                    {move || {
                        open()
                            .then(|| {
                                view! { <TreeDir worktree path=full.clone() depth=depth + 1/> }
                                    .into_any()
                            })
                    }}
                </li>
            }
            .into_any()
        }
        EntryKindDto::File | EntryKindDto::Symlink => {
            let mark_path = full.clone();
            let mark = move || store.status.with(|s| logic::file_mark(s, &mark_path));
            let open_path = full.clone();
            let selected_path = full.clone();
            view! {
                <li>
                    <div
                        class="tree-row file"
                        class:ignored=entry.ignored
                        class:selected=move || {
                            store
                                .open_file
                                .with(|f| {
                                    f.as_ref()
                                        .is_some_and(|f| f.worktree == worktree && f.path == selected_path)
                                })
                        }
                        class:lifted=lifted
                        style:padding-left=padding
                        data-path=full.clone()
                        on:contextmenu=on_menu
                        on:pointerdown=on_press
                        on:click=move |_| {
                            if row_drag.take_click() {
                                return;
                            }
                            store
                                .explorer_selected
                                .set(Some((worktree, open_path.clone(), false)));
                            store
                                .open_file
                                .set(
                                    Some(OpenFile {
                                        worktree,
                                        path: open_path.clone(),
                                    }),
                                );
                            store.show_view(CenterView::Editor);
                        }
                    >
                        <span class="chev"></span>
                        <img
                            class="ficon"
                            alt=""
                            src={
                                let name = name.clone();
                                move || {
                                    store.icons_ready.track();
                                    icons::url(&icons::file_icon(&name, bridge_icons::get))
                                }
                            }
                        />
                        {entry_name(worktree, full.clone(), name, true)}
                        {move || {
                            mark()
                                .map(|m| {
                                    view! { <span class=mark_class(m)>{m.letter().to_string()}</span> }
                                })
                        }}
                    </div>
                </li>
            }
                .into_any()
        }
    }
}
