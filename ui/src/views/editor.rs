//! Editor del archivo abierto (CodeMirror). Ctrl+S guarda; si el archivo cambia en
//! disco y no hay cambios sin guardar, se recarga.

use gryphus_ipc::FileContentDto;
use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::bridge::editor::Editor;
use crate::ipc;
use crate::store::{OpenFile, Store};

#[component]
pub fn CodeEditor() -> impl IntoView {
    let store = expect_context::<Store>();
    let el = NodeRef::<html::Div>::new();
    let editor = StoredValue::new_local(None::<Editor>);
    let (dirty, set_dirty) = signal(false);
    let (notice, set_notice) = signal(None::<String>);
    let (saved_flash, set_saved_flash) = signal(false);

    let load = move |file: OpenFile| {
        spawn_local(async move {
            match ipc::read_file(file.worktree, file.path.clone()).await {
                Ok(FileContentDto::Text { text }) => {
                    set_notice.set(None);
                    editor.with_value(|e| {
                        if let Some(e) = e {
                            e.set_doc(&text, &file.path);
                            e.focus();
                        }
                    });
                }
                Ok(FileContentDto::Binary { size }) => set_notice.set(Some(format!(
                    "Archivo binario ({size} bytes): no se muestra en el editor."
                ))),
                Ok(FileContentDto::TooLarge { size, limit }) => set_notice.set(Some(format!(
                    "Archivo demasiado grande ({size} bytes; límite {limit})."
                ))),
                Err(e) => store.report(e),
            }
        });
    };

    let save = move |text: String| {
        let Some(file) = store.open_file.get_untracked() else {
            return;
        };
        spawn_local(async move {
            match ipc::write_file(file.worktree, file.path, text.clone()).await {
                Ok(()) => {
                    editor.with_value(|e| {
                        if let Some(e) = e {
                            e.mark_saved(&text);
                        }
                    });
                    set_saved_flash.set(true);
                }
                Err(e) => store.report(e),
            }
        });
    };

    // Crea el editor (carga el pegamento JS si falta) y abre el archivo pendiente.
    let creating = StoredValue::new(false);
    Effect::new(move |_| {
        let Some(div) = el.get() else { return };
        if creating.get_value() {
            return;
        }
        creating.set_value(true);
        spawn_local(async move {
            let created = Editor::create(&div, save, move |d| {
                set_dirty.set(d);
                if d {
                    set_saved_flash.set(false);
                }
            })
            .await;
            match created {
                Ok(e) => {
                    editor.set_value(Some(e));
                    if let Some(file) = store.open_file.get_untracked() {
                        load(file);
                    }
                }
                Err(e) => store.error.set(Some(e)),
            }
        });
    });

    Effect::new(move |prev: Option<Option<OpenFile>>| {
        let file = store.open_file.get();
        if prev.as_ref() != Some(&file) {
            if let Some(f) = file.clone() {
                load(f);
            }
        }
        file
    });

    // Recarga si el archivo abierto cambió en disco (p. ej. lo editó un agente).
    Effect::new(move |_| {
        let (worktree, paths) = store.fs_change.get();
        let Some(file) = store.open_file.get_untracked() else {
            return;
        };
        if file.worktree == worktree && paths.contains(&file.path) && !dirty.get_untracked() {
            load(file);
        }
    });

    on_cleanup(move || {
        editor.update_value(|e| {
            if let Some(e) = e.take() {
                e.destroy();
            }
        })
    });

    let save_click = move |_| {
        if let Some(text) = editor.with_value(|e| e.as_ref().map(Editor::text)) {
            save(text);
        }
    };

    view! {
        <div class="view-header">
            <span id="editor-path">
                {move || {
                    store
                        .open_file
                        .get()
                        .map(|f| f.path)
                        .unwrap_or_else(|| "Abre un archivo desde el explorador".into())
                }}
            </span>
            <Show when=move || dirty.get()>
                <span class="dirty" title="Cambios sin guardar">"●"</span>
            </Show>
            <Show when=move || saved_flash.get() && !dirty.get()>
                <span id="editor-saved" class="muted">"guardado"</span>
            </Show>
            <span class="spacer"></span>
            <button
                id="editor-save"
                disabled=move || store.open_file.with(Option::is_none)
                on:click=save_click
            >
                "Guardar (Ctrl+S)"
            </button>
        </div>
        <Show when=move || notice.get().is_some()>
            <p class="hint">{move || notice.get()}</p>
        </Show>
        <div
            class="editor"
            node_ref=el
            style:display=move || if notice.get().is_some() { "none" } else { "block" }
        ></div>
    }
}
