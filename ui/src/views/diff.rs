//! Diff del worktree activo: cambios sin commit o todo lo hecho desde el
//! merge-base con la branch principal. Un agente se revisa por su worktree.

use gryphus_ipc::{DiffDto, FileDiffDto, LineKindDto};
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::bridge::dom;
use crate::bridge::icons as bridge_icons;
use crate::icons;
use crate::ipc;
use crate::layout::CenterView;
use crate::logic;
use crate::store::{OpenFile, Store};

#[component]
pub fn DiffView() -> impl IntoView {
    let store = expect_context::<Store>();
    let (diff, set_diff) = signal(None::<DiffDto>);
    let (from_main, set_from_main) = signal(false);
    let (selected, set_selected) = signal(None::<String>);
    let main_branch = move || {
        store.project.with(|p| {
            p.as_ref()
                .and_then(logic::main_worktree)
                .and_then(|w| w.branch.clone())
        })
    };

    // Solo se aplica la respuesta de la petición más reciente: si no, un diff lento
    // de otro worktree (o de la otra base) podía quedarse en pantalla.
    let req = StoredValue::new(0u64);
    // Memo: reorganizar las columnas sin mostrar u ocultar el diff no lo recarga.
    let shown = Memo::new(move |_| store.center.with(|c| c.is_visible(CenterView::Diff)));
    Effect::new(move |_| {
        store.git_tick.track();
        let visible = shown.get();
        let worktree = store.active_worktree.get();
        let base = if from_main.get() { main_branch() } else { None };
        let (true, Some(worktree)) = (visible, worktree) else {
            return;
        };
        req.update_value(|n| *n += 1);
        let mine = req.get_value();
        spawn_local(async move {
            let res = ipc::git_diff(worktree, base).await;
            if req.try_get_value() != Some(mine) {
                return;
            }
            match res {
                Ok(d) => {
                    let _ = set_diff.try_set(Some(d));
                }
                Err(e) => {
                    // No dejar a la vista el diff anterior como si fuera de este worktree.
                    let _ = set_diff.try_set(None);
                    store.report(e);
                }
            }
        });
    });

    view! {
        <div class="view-header">
            <button
                class="toggle"
                class:on=move || !from_main.get()
                on:click=move |_| set_from_main.set(false)
            >
                "Sin commit"
            </button>
            <button
                id="diff-from-main"
                class="toggle"
                class:on=move || from_main.get()
                on:click=move |_| set_from_main.set(true)
            >
                {move || format!("Desde {}", main_branch().unwrap_or_else(|| "main".into()))}
            </button>
            <span class="spacer"></span>
            <span class="muted">
                {move || {
                    diff.with(|d| d.as_ref().map(|d| format!("{} archivos", d.files.len())))
                }}
            </span>
        </div>
        <div class="diff-body">
            <nav class="diff-files" aria-label="Archivos modificados">
                {move || {
                    let files = diff.with(|d| d.as_ref().map(|d| d.files.clone()).unwrap_or_default());
                    files
                        .into_iter()
                        .map(|file| {
                            let (added, removed) = logic::diff_counts(&file);
                            let (dir, name) = match file.path.rsplit_once('/') {
                                Some((dir, name)) => (dir.to_string(), name.to_string()),
                                None => (String::new(), file.path.clone()),
                            };
                            let path = file.path.clone();
                            let is_selected = {
                                let path = path.clone();
                                move || selected.with(|s| s.as_deref() == Some(path.as_str()))
                            };
                            let target = path.clone();
                            view! {
                                <button
                                    class="diff-files-item"
                                    class:selected=is_selected
                                    title=path.clone()
                                    data-path=path.clone()
                                    on:click=move |_| {
                                        set_selected.set(Some(target.clone()));
                                        dom::reveal(&file_anchor(&target));
                                    }
                                >
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
                                    <span class="diff-files-name ellipsis">
                                        {name}
                                        <span class="diff-files-dir muted">{dir}</span>
                                    </span>
                                    <span class="added">{format!("+{added}")}</span>
                                    <span class="removed">{format!("−{removed}")}</span>
                                </button>
                            }
                        })
                        .collect_view()
                }}
            </nav>
            <div class="diff">
                {move || {
                    let files = diff.with(|d| d.as_ref().map(|d| d.files.clone()).unwrap_or_default());
                    if files.is_empty() {
                        view! { <p class="hint">"Sin cambios."</p> }.into_any()
                    } else {
                        files
                            .into_iter()
                            .map(|file| view! { <DiffFile file/> })
                            .collect_view()
                            .into_any()
                    }
                }}
            </div>
        </div>
    }
}

/// `id` del bloque de un archivo en el diff; la lista lateral salta a él.
fn file_anchor(path: &str) -> String {
    format!("diff-file:{path}")
}

#[component]
fn DiffFile(file: FileDiffDto) -> impl IntoView {
    let store = expect_context::<Store>();
    let (added, removed) = logic::diff_counts(&file);
    let path = file.path.clone();
    let open = move |_| {
        if let Some(worktree) = store.active_worktree.get_untracked() {
            store.open_file.set(Some(OpenFile {
                worktree,
                path: path.clone(),
            }));
            store.show_view(CenterView::Editor);
        }
    };

    view! {
        <details class="diff-file" open=true id=file_anchor(&file.path) data-path=file.path.clone()>
            <summary>
                <span class="path" on:click=open title="Abrir en el editor">
                    {file.path.clone()}
                </span>
                <span class="added">{format!("+{added}")}</span>
                <span class="removed">{format!("−{removed}")}</span>
            </summary>
            {if file.binary {
                view! { <p class="hint">"Archivo binario"</p> }.into_any()
            } else {
                file.hunks
                    .into_iter()
                    .map(|hunk| {
                        view! {
                            <div class="hunk">
                                <div class="hunk-header">
                                    {format!(
                                        "@@ -{},{} +{},{} @@",
                                        hunk.old_start,
                                        hunk.old_lines,
                                        hunk.new_start,
                                        hunk.new_lines,
                                    )}
                                </div>
                                {hunk
                                    .lines
                                    .into_iter()
                                    .map(|line| {
                                        let (class, sign) = match line.kind {
                                            LineKindDto::Added => ("line added", "+"),
                                            LineKindDto::Removed => ("line removed", "-"),
                                            LineKindDto::Context => ("line", " "),
                                        };
                                        view! { <div class=class>{format!("{sign} {}", line.text)}</div> }
                                    })
                                    .collect_view()}
                            </div>
                        }
                    })
                    .collect_view()
                    .into_any()
            }}
        </details>
    }
}
