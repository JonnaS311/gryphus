//! Editor con pestañas (CodeMirror). Cada archivo abierto es una pestaña con su propio
//! documento (cursor e historial de deshacer). Ctrl+S guarda, Ctrl+W cierra la pestaña
//! (pregunta si hay cambios sin guardar) y Ctrl+Tab / Ctrl+Shift+Tab cambian de pestaña.
//! En macOS son ⌘S y ⌘W; Ctrl+Tab no cambia.
//! Si un archivo abierto cambia en disco y no tiene cambios sin guardar, se recarga.

use std::collections::HashMap;

use gryphus_ipc::FileContentDto;
use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::bridge::editor::Editor;
use crate::bridge::icons as bridge_icons;
use crate::icons;
use crate::ipc;
use crate::layout::CenterView;
use crate::logic;
use crate::store::{OpenFile, SaveAnswer, Store};

#[component]
pub fn CodeEditor() -> impl IntoView {
    let store = expect_context::<Store>();
    let el = NodeRef::<html::Div>::new();
    let editor = StoredValue::new_local(None::<Editor>);
    // Archivos que no se pueden editar (binarios, demasiado grandes): aviso en lugar de documento.
    let notices = RwSignal::new(HashMap::<OpenFile, String>::new());
    let (saved_flash, set_saved_flash) = signal(false);

    let active_dirty = move || {
        store.open_file.with(|f| {
            f.as_ref()
                .is_some_and(|f| store.dirty_files.with(|d| d.contains(f)))
        })
    };
    let notice = move || {
        store
            .open_file
            .with(|f| f.as_ref().and_then(|f| notices.with(|n| n.get(f).cloned())))
    };

    // Muestra en CodeMirror la pestaña activa, si ya tiene documento.
    let sync_view = move || {
        let active = store.open_file.get_untracked();
        editor.with_value(|e| {
            let Some(e) = e else { return };
            match active.map(|f| f.key()) {
                Some(key) if e.has_doc(&key) => {
                    e.show_doc(&key);
                    e.focus();
                }
                _ => e.clear(),
            }
        });
    };

    let load = move |file: OpenFile| {
        spawn_local(async move {
            let result = ipc::read_file(file.worktree, file.path.clone()).await;
            // Se cerró la pestaña (o el editor, al cerrar el proyecto) mientras se leía.
            let still_open = store
                .open_files
                .try_with_untracked(|t| t.contains(&file))
                .unwrap_or(false);
            if !still_open || notices.is_disposed() {
                return;
            }
            let blocked = |msg: String| {
                notices.update(|n| {
                    n.insert(file.clone(), msg);
                });
            };
            match result {
                Ok(FileContentDto::Text { text }) => {
                    notices.update(|n| {
                        n.remove(&file);
                    });
                    editor.with_value(|e| {
                        if let Some(e) = e {
                            e.open_doc(&file.key(), &text, file.name());
                        }
                    });
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
            sync_view();
        });
    };

    // Escribe a disco el texto de una pestaña; `true` si se guardó.
    let write = move |file: OpenFile, text: String| async move {
        match ipc::write_file(file.worktree, file.path.clone(), text.clone()).await {
            Ok(()) => {
                editor.with_value(|e| {
                    if let Some(e) = e {
                        e.mark_saved(&file.key(), &text);
                    }
                });
                if store.open_file.get_untracked().as_ref() == Some(&file) {
                    set_saved_flash.set(true);
                }
                true
            }
            Err(e) => {
                store.report(e);
                false
            }
        }
    };

    // Guarda la pestaña activa con el texto de SU documento, no el de la vista: si aún
    // carga, es binaria o no se pudo leer, no hay documento y no se escribe nada (el
    // texto de un archivo nunca debe acabar en la ruta de otro).
    let save_active = move || {
        let Some(file) = store.open_file.get_untracked() else {
            return;
        };
        let text = editor.with_value(|e| e.as_ref().and_then(|e| e.text(&file.key())));
        if let Some(text) = text {
            spawn_local(async move {
                write(file, text).await;
            });
        }
    };

    // Cierra una pestaña; si tiene cambios pregunta Guardar / No guardar / Cancelar.
    let close = move |file: OpenFile| {
        spawn_local(async move {
            if store.dirty_files.with_untracked(|d| d.contains(&file)) {
                store.open_file.set(Some(file.clone()));
                let answer = store
                    .ask_save(format!(
                        "¿Guardar los cambios de {} antes de cerrarlo?\nSi no los guardas, se perderán.",
                        file.name()
                    ))
                    .await;
                match answer {
                    SaveAnswer::Cancel => return,
                    SaveAnswer::Discard => {}
                    SaveAnswer::Save => {
                        let text =
                            editor.with_value(|e| e.as_ref().and_then(|e| e.text(&file.key())));
                        let Some(text) = text else { return };
                        if !write(file.clone(), text).await {
                            return;
                        }
                    }
                }
            }
            let Some(idx) = store
                .open_files
                .with_untracked(|t| t.iter().position(|f| f == &file))
            else {
                return;
            };
            store.open_files.update(|t| {
                t.remove(idx);
            });
            if store.open_file.get_untracked().as_ref() == Some(&file) {
                let next = store
                    .open_files
                    .with_untracked(|t| logic::tab_after_close(t.len(), idx).map(|i| t[i].clone()));
                store.open_file.set(next);
            }
        });
    };

    // Pasa a la pestaña siguiente (`step` = 1) o anterior (`step` = -1).
    let cycle = move |step: isize| {
        let next = store.open_files.with_untracked(|t| {
            if t.is_empty() {
                return None;
            }
            let len = t.len() as isize;
            let current = store
                .open_file
                .with_untracked(|a| a.as_ref().and_then(|a| t.iter().position(|f| f == a)))
                .map_or(0, |i| i as isize);
            Some(t[(current + step).rem_euclid(len) as usize].clone())
        });
        if next.is_some() {
            store.open_file.set(next);
        }
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
            let created = Editor::create(
                &div,
                move |_| save_active(),
                move |key: String, dirty: bool| {
                    let file = store
                        .open_files
                        .with_untracked(|t| t.iter().find(|f| f.key() == key).cloned());
                    let Some(file) = file else { return };
                    if dirty && store.open_file.get_untracked().as_ref() == Some(&file) {
                        set_saved_flash.set(false);
                    }
                    store.dirty_files.update(|d| {
                        if dirty {
                            d.insert(file);
                        } else {
                            d.remove(&file);
                        }
                    });
                },
            )
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

    // Asignar `open_file` (explorador, diff, pestañas) abre la pestaña si hace falta y la muestra.
    Effect::new(move |_| {
        let file = store.open_file.get();
        set_saved_flash.set(false);
        let Some(file) = file else {
            sync_view();
            return;
        };
        if !store.open_files.with_untracked(|t| t.contains(&file)) {
            store.open_files.update(|t| t.push(file.clone()));
        }
        let has_doc = editor.with_value(|e| e.as_ref().map(|e| e.has_doc(&file.key())));
        match has_doc {
            None => {} // el editor aún se está creando: lo carga al terminar
            Some(true) => sync_view(),
            Some(false) if notices.with_untracked(|n| n.contains_key(&file)) => sync_view(),
            Some(false) => {
                sync_view(); // vista vacía mientras carga: no se ve ni se edita el anterior
                load(file);
            }
        }
    });

    // Libera los documentos de las pestañas cerradas (también al cerrar el proyecto).
    Effect::new(move |prev: Option<Vec<OpenFile>>| {
        let now = store.open_files.get();
        if let Some(prev) = prev {
            for file in prev.iter().filter(|f| !now.contains(f)) {
                editor.with_value(|e| {
                    if let Some(e) = e {
                        e.close_doc(&file.key());
                    }
                });
                notices.update(|n| {
                    n.remove(file);
                });
                store.dirty_files.update(|d| {
                    d.remove(file);
                });
            }
        }
        now
    });

    // Recarga las pestañas sin cambios cuyo archivo cambió en disco (p. ej. lo editó un agente).
    Effect::new(move |_| {
        let (worktree, paths) = store.fs_change.get();
        let changed: Vec<OpenFile> = store.open_files.with_untracked(|t| {
            t.iter()
                .filter(|f| f.worktree == worktree && paths.contains(&f.path))
                .filter(|f| !store.dirty_files.with_untracked(|d| d.contains(*f)))
                .cloned()
                .collect()
        });
        for file in changed {
            spawn_local(async move {
                // Si se borró o ya no es texto, la pestaña conserva lo último que se leyó.
                let Ok(FileContentDto::Text { text }) =
                    ipc::read_file(file.worktree, file.path.clone()).await
                else {
                    return;
                };
                if notices.is_disposed() || store.dirty_files.with_untracked(|d| d.contains(&file))
                {
                    return;
                }
                editor.with_value(|e| {
                    if let Some(e) = e {
                        e.reload_doc(&file.key(), &text);
                    }
                });
            });
        }
    });

    let _keys = window_event_listener(leptos::ev::keydown, move |ev| {
        if store.view.get_untracked() != CenterView::Editor
            || store.confirm.with_untracked(Option::is_some)
        {
            return;
        }
        // Cerrar usa el modificador principal (⌘W en macOS); cambiar de pestaña
        // es Ctrl+Tab en todas partes (⌘Tab es del sistema en macOS).
        match ev.key().as_str() {
            "w" | "W" if crate::bridge::dom::primary_mod(&ev) => {
                if let Some(file) = store.open_file.get_untracked() {
                    close(file);
                }
            }
            "Tab" if ev.ctrl_key() => cycle(if ev.shift_key() { -1 } else { 1 }),
            _ => return,
        }
        ev.prevent_default();
    });

    on_cleanup(move || {
        editor.update_value(|e| {
            if let Some(e) = e.take() {
                e.destroy();
            }
        })
    });

    let save_click = move |_| save_active();

    view! {
        <Show when=move || store.open_files.with(|t| !t.is_empty())>
            <div class="editor-tabs" role="tablist" aria-label="Archivos abiertos">
                <For
                    each=move || store.open_files.get()
                    key=|f| f.clone()
                    children=move |file| view! { <EditorTab file close/> }
                />
            </div>
        </Show>
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
            <Show when=active_dirty>
                <span class="dirty" title="Cambios sin guardar">"●"</span>
            </Show>
            <Show when=move || saved_flash.get() && !active_dirty()>
                <span id="editor-saved" class="muted">"guardado"</span>
            </Show>
            <span class="spacer"></span>
            <button
                id="editor-save"
                disabled=move || store.open_file.with(Option::is_none) || notice().is_some()
                on:click=save_click
            >
                {if crate::bridge::dom::is_mac() { "Guardar (⌘S)" } else { "Guardar (Ctrl+S)" }}
            </button>
        </div>
        <Show when=move || notice().is_some()>
            <p class="hint">{notice}</p>
        </Show>
        <div
            class="editor"
            node_ref=el
            style:display=move || {
                if notice().is_some() || store.open_file.with(Option::is_none) {
                    "none"
                } else {
                    "block"
                }
            }
        ></div>
    }
}

#[component]
fn EditorTab<C>(file: OpenFile, close: C) -> impl IntoView
where
    C: Fn(OpenFile) + Copy + 'static,
{
    let store = expect_context::<Store>();
    let name = file.name().to_owned();
    let path = file.path.clone();

    let is_active = Memo::new({
        let file = file.clone();
        move |_| store.open_file.with(|a| a.as_ref() == Some(&file))
    });
    let is_dirty = Memo::new({
        let file = file.clone();
        move |_| store.dirty_files.with(|d| d.contains(&file))
    });
    let hint = {
        let path = path.clone();
        move || {
            store
                .open_files
                .with(|t| logic::tab_hint(t.iter().map(|f| f.path.as_str()), &path))
        }
    };
    let title = {
        let path = path.clone();
        let worktree = file.worktree;
        move || {
            let label = store.project.with(|p| {
                p.as_ref()
                    .and_then(|p| logic::worktree(p, worktree))
                    .map(logic::worktree_label)
            });
            match label {
                Some(label) => format!("{path} — {label}"),
                None => path.clone(),
            }
        }
    };
    let activate = {
        let file = file.clone();
        move |_| store.open_file.set(Some(file.clone()))
    };
    let close_middle = {
        let file = file.clone();
        move |ev: leptos::ev::MouseEvent| {
            if ev.button() == 1 {
                ev.prevent_default();
                close(file.clone());
            }
        }
    };
    let close_click = move |ev: leptos::ev::MouseEvent| {
        ev.stop_propagation();
        close(file.clone());
    };

    view! {
        <div
            class="editor-tab"
            class:active=is_active
            class:dirty=is_dirty
            role="tab"
            aria-selected=move || is_active.get().to_string()
            title=title
            data-path=path
            on:click=activate
            on:auxclick=close_middle
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
            <span class="editor-tab-name">{name}</span>
            {move || hint().map(|h| view! { <span class="editor-tab-hint">{h}</span> })}
            <button
                class="editor-tab-close"
                title=move || {
                    match (is_dirty.get(), crate::bridge::dom::is_mac()) {
                        (true, true) => "Cambios sin guardar — cerrar (⌘W)",
                        (true, false) => "Cambios sin guardar — cerrar (Ctrl+W)",
                        (false, true) => "Cerrar (⌘W)",
                        (false, false) => "Cerrar (Ctrl+W)",
                    }
                }
                aria-label="Cerrar"
                on:click=close_click
            >
                <span class="editor-tab-dot">"●"</span>
                <span class="editor-tab-x">"×"</span>
            </button>
        </div>
    }
}
