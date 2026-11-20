//! Panel derecho: árbol perezoso del worktree activo (un nivel por expansión).

use gryphus_ipc::{DirEntryDto, EntryKindDto, FileStateDto};
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::ipc;
use crate::layout::CenterView;
use crate::logic;
use crate::store::{OpenFile, Store};

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

    view! {
        <aside class="pane pane-right">
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

#[component]
fn TreeEntry(worktree: u64, entry: DirEntryDto, parent: String, depth: usize) -> impl IntoView {
    let store = expect_context::<Store>();
    let full = logic::join_path(&parent, &entry.name);
    let padding = format!("{}px", 8 + depth * 12);
    let name = entry.name.clone();

    match entry.kind {
        EntryKindDto::Dir => {
            let (open_key, chev_key) = (full.clone(), full.clone());
            let open = move || store.is_expanded(worktree, &open_key);
            let chev_open = move || store.is_expanded(worktree, &chev_key);
            let toggle_key = full.clone();
            let changed_path = full.clone();
            let changed = move || store.status.with(|s| logic::dir_changed(s, &changed_path));
            view! {
                <li>
                    <div
                        class="tree-row dir"
                        class:ignored=entry.ignored
                        style:padding-left=padding
                        on:click=move |_| store.toggle_expanded(worktree, &toggle_key)
                    >
                        <span class="chev">{move || if chev_open() { "▾" } else { "▸" }}</span>
                        <span class="name">{name}</span>
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
                        on:click=move |_| {
                            store
                                .open_file
                                .set(
                                    Some(OpenFile {
                                        worktree,
                                        path: open_path.clone(),
                                    }),
                                );
                            store.view.set(CenterView::Editor);
                        }
                    >
                        <span class="chev"></span>
                        <span class="name">{name}</span>
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
