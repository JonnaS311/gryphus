//! Raíz de la UI: bienvenida o espacio de trabajo, eventos y atajos globales.

use gryphus_ipc::{event, AgentStatusEvent, FeatureEvent, FilesChangedEvent, WorktreeEvent};
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::bridge::tauri::listen;
use crate::ipc;
use crate::layout::CenterView;
use crate::panels::{AgentsPanel, Explorer, Navbar, StatusBar};
use crate::store::Store;
use crate::views::{MainView, Welcome};

#[component]
pub fn App() -> impl IntoView {
    let store = Store::new();
    provide_context(store);
    store.load_settings();
    crate::bridge::perf::mark("ui-ready");
    spawn_local(async move {
        let _ = ipc::ui_ready().await;
        // Tablas de iconos y xterm/CodeMirror, fuera del camino crítico del arranque.
        if crate::bridge::icons::load().await.is_ok() {
            store.icons_ready.set(true);
        }
        let _ = crate::bridge::glue::ensure_loaded().await;
    });
    store.refresh_project();
    register_events(store);

    // Guarda de cierre: con agentes activos o cambios sin guardar, la X de la
    // ventana pregunta en vez de detenerlos/descartarlos en silencio.
    Effect::new(move |prev: Option<bool>| {
        let active = store.project.with(|p| {
            p.as_ref()
                .is_some_and(|p| p.agents.iter().any(|a| a.status.is_active()))
        });
        let guard = active || store.dirty_files.with(|d| !d.is_empty());
        if prev != Some(guard) {
            spawn_local(async move {
                let _ = ipc::set_close_guard(guard).await;
            });
        }
        guard
    });
    listen(event::CLOSE_REQUESTED, move |_: ()| {
        spawn_local(async move {
            let proceed = match store.close_warning() {
                Some(warning) => store.ask(warning).await,
                None => true,
            };
            if proceed {
                if let Err(e) = ipc::confirm_close().await {
                    store.report(e);
                }
            }
        });
    });

    let _keys = window_event_listener(leptos::ev::keydown, move |ev| {
        if !ev.ctrl_key() {
            return;
        }
        let view = match ev.key().as_str() {
            "1" => CenterView::Agent,
            "2" => CenterView::Editor,
            "3" => CenterView::Diff,
            _ => return,
        };
        ev.prevent_default();
        store.view.set(view);
    });

    view! {
        <div class="app">
            <Navbar/>
            <main class="app-body">
                <Show
                    when=move || store.loaded.get()
                    fallback=|| view! { <div class="loading">"Cargando…"</div> }
                >
                    <Show
                        when=move || store.project.with(Option::is_some)
                        fallback=|| view! { <Welcome/> }
                    >
                        <Workspace/>
                    </Show>
                </Show>
            </main>
        </div>
        <ConfirmDialog/>
        <ErrorToast/>
    }
}

fn register_events(store: Store) {
    let refresh = move |_: FeatureEvent| store.refresh_project();
    listen(event::FEATURE_CHANGED, refresh);
    listen(event::FEATURE_REMOVED, refresh);
    listen(event::WORKTREES_CHANGED, move |_: ()| {
        store.refresh_project()
    });
    listen(event::AGENT_STATUS, move |e: AgentStatusEvent| {
        store.project.update(|p| {
            if let Some(agent) = p
                .as_mut()
                .and_then(|p| p.agents.iter_mut().find(|a| a.id == e.agent))
            {
                agent.status = e.status;
            }
        });
        // El evento puede llegar antes que el agente (recién lanzado) esté en la
        // caché, o mientras un `get_project` anterior al cambio sigue en vuelo y lo
        // pisaría al volver: se pide el estado de nuevo (los cambios de estado son raros).
        store.refresh_project();
    });
    listen(event::FILES_CHANGED, move |e: FilesChangedEvent| {
        store.fs_change.set((e.worktree, e.paths));
        store.fs_tick.update(|t| *t += 1);
    });
    listen(event::GIT_CHANGED, move |e: WorktreeEvent| {
        if store.active_worktree.get_untracked() == Some(e.worktree) {
            store.bump_git();
        }
    });
}

#[component]
fn Workspace() -> impl IntoView {
    let store = expect_context::<Store>();

    // Status y resumen git del worktree activo; se recargan con cada `git:changed`.
    // Las respuestas pueden llegar desordenadas (ráfagas de eventos, cambio de
    // worktree): solo se aplica la de la petición más reciente.
    let req = StoredValue::new(0u64);
    Effect::new(move |_| {
        store.git_tick.track();
        let worktree = store.active_worktree.get();
        req.update_value(|n| *n += 1);
        let mine = req.get_value();
        // `try_`: el espacio de trabajo pudo desmontarse (proyecto cerrado) mientras tanto.
        let current = move || req.try_get_value() == Some(mine);
        spawn_local(async move {
            if let Some(wt) = worktree {
                let res = ipc::git_status(wt).await;
                if !current() {
                    return;
                }
                match res {
                    Ok(s) => store.status.set(s),
                    Err(e) if e.code == "WORKTREE_MISSING" => store.status.set(Vec::new()),
                    Err(e) => {
                        store.status.set(Vec::new());
                        store.report(e);
                    }
                }
            }
            if let Ok(o) = ipc::git_overview().await {
                if current() {
                    store.overview.set(Some(o));
                }
            }
        });
    });

    view! {
        <div class="shell">
            <AgentsPanel/>
            <MainView/>
            <Explorer/>
            <StatusBar/>
        </div>
    }
}

/// Modal de confirmación con el tema de la app (ver `Store::ask`).
#[component]
fn ConfirmDialog() -> impl IntoView {
    let store = expect_context::<Store>();
    let _keys = window_event_listener(leptos::ev::keydown, move |ev| {
        if store.confirm.with_untracked(|c| c.is_none()) {
            return;
        }
        match ev.key().as_str() {
            "Escape" => store.answer(None),
            "Enter" => store.answer(Some(0)),
            _ => return,
        }
        ev.prevent_default();
        ev.stop_propagation();
    });
    move || {
        store
            .confirm
            .with(|c| c.as_ref().map(|c| (c.message.clone(), c.choices.clone())))
            .map(|(message, choices)| {
                view! {
                    <div class="backdrop over" on:click=move |_| store.answer(None)></div>
                    <section class="confirm" role="alertdialog" aria-modal="true">
                        <p class="confirm-message">{message}</p>
                        <div class="confirm-actions">
                            <button id="confirm-cancel" on:click=move |_| store.answer(None)>
                                "Cancelar"
                            </button>
                            {choices
                                .into_iter()
                                .enumerate()
                                .rev()
                                .map(|(i, label)| {
                                    let primary = i == 0;
                                    view! {
                                        <button
                                            id=if primary {
                                                "confirm-accept".to_owned()
                                            } else {
                                                format!("confirm-choice-{i}")
                                            }
                                            class:primary=primary
                                            autofocus=primary
                                            on:click=move |_| store.answer(Some(i))
                                        >
                                            {label}
                                        </button>
                                    }
                                })
                                .collect_view()}
                        </div>
                    </section>
                }
            })
    }
}

#[component]
fn ErrorToast() -> impl IntoView {
    let store = expect_context::<Store>();
    move || {
        store.error.get().map(|message| {
            view! {
                <div class="toast" role="alert" on:click=move |_| store.error.set(None)>
                    {message}
                    <span class="muted">" (clic para cerrar)"</span>
                </div>
            }
        })
    }
}
