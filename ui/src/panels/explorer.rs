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
use crate::store::{FileDrop, MenuTarget, OpenFile, RenameTarget, Store};

#[component]
pub fn Explorer() -> impl IntoView {
    let store = expect_context::<Store>();
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
                        style:padding-left=padding
                        data-path=full.clone()
                        on:click=move |_| {
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
                        style:padding-left=padding
                        data-path=full.clone()
                        on:contextmenu=on_menu
                        on:click=move |_| {
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
