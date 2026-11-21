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
    // Archivo cuyo contenido tiene el editor. Guardar solo se permite sobre él: el
    // texto de un archivo nunca debe escribirse en la ruta de otro.
    let shown = StoredValue::new(None::<OpenFile>);

    // `reload`: recarga por un cambio en disco (sin robar el foco ni perder el
    // cursor si el contenido no cambió, p. ej. tras nuestro propio guardado).
    let load = move |file: OpenFile, reload: bool| {
        spawn_local(async move {
            let res = ipc::read_file(file.worktree, file.path.clone()).await;
            // Respuesta tardía de un archivo que ya no es el abierto (clics rápidos):
            // aplicarla mostraría A bajo el nombre de B y Ctrl+S escribiría A en B.
            if store.open_file.try_get_untracked().flatten().as_ref() != Some(&file) {
                return;
            }
            let blocked = |msg: String| {
                shown.set_value(None);
                set_notice.set(Some(msg));
            };
            match res {
                Ok(FileContentDto::Text { text }) => {
                    let ready = editor.with_value(|e| {
                        let Some(e) = e else { return false };
                        if !(reload && e.text() == text) {
                            e.set_doc(&text, &file.path);
                        }
                        if !reload {
                            e.focus();
                        }
                        true
                    });
                    if ready {
                        set_notice.set(None);
                        shown.set_value(Some(file));
                    }
                }
                Ok(FileContentDto::Binary { size }) => blocked(format!(
                    "Archivo binario ({size} bytes): no se muestra en el editor."
                )),
                Ok(FileContentDto::TooLarge { size, limit }) => blocked(format!(
                    "Archivo demasiado grande ({size} bytes; límite {limit})."
                )),
                Err(e) => {
                    blocked(format!("No se pudo abrir {}: {}", file.path, e.message));
                    store.report(e);
                }
            }
        });
    };

    let save = move |text: String| {
        let Some(file) = store.open_file.get_untracked() else {
            return;
        };
        // Binario, demasiado grande, ilegible o aún cargando: el editor conserva el
        // texto del archivo anterior y guardarlo lo sobrescribiría.
        if shown.get_value().as_ref() != Some(&file) {
            return;
        }
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
                store.editor_dirty.set(d);
                if d {
                    set_saved_flash.set(false);
                }
            })
            .await;
            match created {
                Ok(e) => {
                    editor.set_value(Some(e));
                    if let Some(file) = store.open_file.get_untracked() {
                        load(file, false);
                    }
                }
                Err(e) => store.error.set(Some(e)),
            }
        });
    });

    // Abrir otro archivo con cambios sin guardar los descartaba sin avisar.
    Effect::new(move |_| {
        let file = store.open_file.get();
        let current = shown.get_value();
        let Some(f) = file.filter(|f| Some(f) != current.as_ref()) else {
            return;
        };
        if dirty.get_untracked() && current.is_some() {
            spawn_local(async move {
                let name = current.as_ref().map(|c| c.path.clone()).unwrap_or_default();
                if store
                    .ask(format!("{name} tiene cambios sin guardar. ¿Descartarlos?"))
                    .await
                {
                    load(f, false);
                } else {
                    // Vuelve al archivo con cambios (el efecto lo ve igual a `shown`).
                    store.open_file.set(current);
                }
            });
        } else {
            load(f, false);
        }
    });

    // Recarga si el archivo abierto cambió en disco (p. ej. lo editó un agente).
    Effect::new(move |_| {
        let (worktree, paths) = store.fs_change.get();
        let Some(file) = store.open_file.get_untracked() else {
            return;
        };
        if file.worktree == worktree && paths.contains(&file.path) && !dirty.get_untracked() {
            load(file, true);
        }
    });

    on_cleanup(move || {
        let _ = store.editor_dirty.try_set(false);
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
                disabled=move || store.open_file.with(Option::is_none) || notice.get().is_some()
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
