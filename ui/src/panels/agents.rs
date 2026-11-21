//! Panel izquierdo: features → worktrees y agentes.

use gryphus_ipc::{AgentDto, FeatureDto, WorktreeChoiceDto};
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::ipc;
use crate::logic;
use crate::panels::InlineRename;
use crate::store::{MenuTarget, RenameTarget, Store};

#[component]
pub fn AgentsPanel() -> impl IntoView {
    let store = expect_context::<Store>();
    let (creating, set_creating) = signal(false);
    let features = move || {
        store
            .project
            .with(|p| p.as_ref().map(|p| p.features.clone()).unwrap_or_default())
    };

    view! {
        <aside class="pane pane-left">
            <header class="pane-header">
                <h2 class="pane-title">"Features"</h2>
                <button
                    id="new-feature"
                    class="icon"
                    title="Nueva feature"
                    on:click=move |_| set_creating.update(|c| *c = !*c)
                >
                    "+"
                </button>
            </header>
            <Show when=move || creating.get()>
                <NewFeatureForm open=set_creating/>
            </Show>
            <Show when=move || features().is_empty() && !creating.get()>
                <p class="hint">"Crea una feature con + para empezar."</p>
            </Show>
            // Con clave: cada refresco del proyecto (p. ej. un cambio de estado de un
            // agente) recreaba todas las features y reiniciaba lo elegido en
            // "Lanzar agente" (proveedor y worktree) mientras el usuario lo usaba.
            <For
                each=features
                key=|f| (f.id, f.worktrees.clone())
                children=|feature| view! { <FeatureItem feature/> }
            />
        </aside>
    }
}

#[component]
fn NewFeatureForm(open: WriteSignal<bool>) -> impl IntoView {
    let store = expect_context::<Store>();
    let (name, set_name) = signal(String::new());
    let (choice, set_choice) = signal(String::from("new"));
    let (base, set_base) = signal(String::new());
    let free = move || {
        store.project.with(|p| {
            p.as_ref()
                .map(|p| {
                    logic::unassigned_worktrees(p)
                        .into_iter()
                        .map(|w| (w.id, logic::worktree_label(w)))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        })
    };
    let submit = move |ev: leptos::ev::SubmitEvent| {
        ev.prevent_default();
        let name = name.get_untracked();
        let worktree = match choice.get_untracked().parse::<u64>() {
            Ok(id) => WorktreeChoiceDto::Existing { id },
            Err(_) => WorktreeChoiceDto::New {
                base: Some(base.get_untracked()),
            },
        };
        let initial = store
            .settings
            .with_untracked(|s| s.initial_agent().map(str::to_owned));
        spawn_local(async move {
            match ipc::create_feature(name, worktree).await {
                Ok(id) => {
                    open.set(false);
                    store.selected_feature.set(Some(id));
                    match initial {
                        Some(provider) => launch_initial_agent(store, id, provider).await,
                        None => store.refresh_project(),
                    }
                }
                Err(e) => store.report(e),
            }
        });
    };

    view! {
        <form class="new-feature" on:submit=submit>
            <input
                id="feature-name"
                placeholder="nombre (p. ej. login)"
                required=true
                prop:value=name
                on:input=move |ev| set_name.set(event_target_value(&ev))
            />
            <select id="feature-worktree" on:change=move |ev| set_choice.set(event_target_value(&ev))>
                <option value="new" selected=true>"Worktree nuevo (feat/nombre)"</option>
                {move || {
                    free()
                        .into_iter()
                        .map(|(id, label)| {
                            view! { <option value=id.to_string()>{format!("Usar {label}")}</option> }
                        })
                        .collect_view()
                }}
            </select>
            <Show when=move || choice.get() == "new">
                <input
                    placeholder="desde (opcional: branch base)"
                    prop:value=base
                    on:input=move |ev| set_base.set(event_target_value(&ev))
                />
            </Show>
            <div class="row">
                <button id="create-feature" type="submit" class="primary">"Crear"</button>
                <button type="button" on:click=move |_| open.set(false)>"Cancelar"</button>
            </div>
        </form>
    }
}

/// Lanza el agente inicial de Configuración en el primer worktree de la feature recién creada.
async fn launch_initial_agent(store: Store, feature: u64, provider: String) {
    let project = match ipc::get_project().await {
        Ok(Some(p)) => p,
        Ok(None) => return store.refresh_project(),
        Err(e) => {
            store.report(e);
            return store.refresh_project();
        }
    };
    let worktree = logic::feature(&project, feature).and_then(|f| f.worktrees.first().copied());
    let available = project
        .providers
        .iter()
        .any(|p| p.id == provider && p.available);
    let Some(worktree) = worktree else {
        return store.refresh_project();
    };
    if !available {
        store.error.set(Some(format!(
            "El agente inicial «{provider}» no está instalado; la feature se creó sin agente."
        )));
        return store.refresh_project();
    }
    let result = ipc::spawn_agent(feature, worktree, provider).await;
    store.refresh_project();
    match result {
        Ok(agent) => store.select_agent(agent, worktree, feature),
        Err(e) => store.report(e),
    }
}

fn delete_feature(store: Store, id: u64) {
    spawn_local(async move {
        // Una sola pregunta con tres salidas: antes, "Cancelar" en la segunda
        // pregunta (worktrees) no abortaba, sino que borraba conservando worktrees.
        let remove = match store
            .choose(
                "¿Eliminar esta feature? Las branches se conservan en git.",
                [
                    "Eliminar con sus worktrees",
                    "Eliminar y conservar worktrees",
                ],
            )
            .await
        {
            Some(0) => true,
            Some(_) => false,
            None => return,
        };
        let mut result = ipc::delete_feature(id, remove, false).await;
        if matches!(&result, Err(e) if e.code == "CONFLICT")
            && store
                .ask("Hay cambios sin commit en sus worktrees. ¿Eliminar de todas formas?")
                .await
        {
            result = ipc::delete_feature(id, remove, true).await;
        }
        match result {
            Ok(()) => store.refresh_project(),
            Err(e) => store.report(e),
        }
    });
}

#[component]
fn FeatureItem(feature: FeatureDto) -> impl IntoView {
    let store = expect_context::<Store>();
    let id = feature.id;
    // Según su posición en la lista: cada feature tiene un color distinto al de sus vecinas.
    // Reactivo porque `<For>` conserva el componente al cambiar el orden.
    let hue = move || {
        store.project.with(|p| {
            let index = p
                .as_ref()
                .and_then(|p| p.features.iter().position(|f| f.id == id))
                .unwrap_or(0);
            logic::feature_hue(index)
        })
    };
    let collapsed = move || store.is_collapsed(id);
    let agent_count = move || {
        store
            .project
            .with(|p| p.as_ref().map_or(0, |p| logic::agents_of(p, id).len()))
    };
    let show_count = move || collapsed() && agent_count() != 0;
    let agents = move || {
        store.project.with(|p| {
            p.as_ref()
                .map(|p| {
                    logic::agents_of(p, id)
                        .into_iter()
                        .cloned()
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        })
    };
    let worktrees = feature.worktrees.clone();

    view! {
        <section
            class="feature"
            class:selected=move || store.selected_feature.get() == Some(id)
            class:collapsed=collapsed
            style=move || format!("--feature-hue: {}", hue())
            data-feature=feature.name.clone()
        >
            <header
                class="feature-header"
                title="Clic para expandir o contraer"
                aria-expanded=move || (!collapsed()).to_string()
                on:click=move |_| {
                    store.selected_feature.set(Some(id));
                    store.toggle_collapsed(id);
                }
            >
                <span class="chev">{move || if collapsed() { "▸" } else { "▾" }}</span>
                <span class="feature-name">{feature.name.clone()}</span>
                <Show when=show_count>
                    <span class="feature-count" title="Agentes">{agent_count}</span>
                </Show>
                <button
                    class="icon danger"
                    title="Eliminar feature"
                    on:click=move |ev| {
                        ev.stop_propagation();
                        delete_feature(store, id);
                    }
                >
                    "×"
                </button>
            </header>
            <Show when=move || !collapsed()>
                <ul class="worktrees">
                    {worktrees.iter().map(|wt| view! { <WorktreeRow worktree=*wt/> }).collect_view()}
                </ul>
                <ul class="agents">
                    {move || agents().into_iter().map(|agent| view! { <AgentRow agent/> }).collect_view()}
                </ul>
                <NewAgent feature=id worktrees=worktrees.clone()/>
            </Show>
        </section>
    }
}

#[component]
fn WorktreeRow(worktree: u64) -> impl IntoView {
    let store = expect_context::<Store>();
    let label = move || {
        store.project.with(|p| {
            p.as_ref()
                .and_then(|p| logic::worktree(p, worktree))
                .map(logic::worktree_label)
                .unwrap_or_default()
        })
    };
    view! {
        <li
            class="worktree"
            class:active=move || store.active_worktree.get() == Some(worktree)
            title="Ver archivos y cambios de este worktree"
            on:click=move |_| store.active_worktree.set(Some(worktree))
        >
            <span class="glyph">"⎇"</span>
            {label}
        </li>
    }
}

#[component]
fn AgentRow(agent: AgentDto) -> impl IntoView {
    let store = expect_context::<Store>();
    let AgentDto {
        id,
        name,
        feature,
        worktree,
        provider,
        status,
    } = agent;
    let rename_target = RenameTarget::Agent(id);
    let renaming = {
        let t = rename_target.clone();
        move || store.is_renaming(&t)
    };
    let commit = move |new_name: String| {
        spawn_local(async move {
            match ipc::rename_agent(id, new_name).await {
                Ok(()) => store.refresh_project(),
                Err(e) => store.report(e),
            }
        });
    };
    let (class, label) = logic::status_badge(&status);
    let active = status.is_active();
    let stop = move |ev: leptos::ev::MouseEvent| {
        ev.stop_propagation();
        spawn_local(async move {
            if let Err(e) = ipc::stop_agent(id).await {
                store.report(e);
            }
        });
    };
    let restart = move |ev: leptos::ev::MouseEvent| {
        ev.stop_propagation();
        spawn_local(async move {
            match ipc::restart_agent(id).await {
                Ok(()) => store.select_agent(id, worktree, feature),
                Err(e) => store.report(e),
            }
        });
    };
    let remove = move |ev: leptos::ev::MouseEvent| {
        ev.stop_propagation();
        spawn_local(async move {
            if !store.ask("¿Quitar este agente de la lista?").await {
                return;
            }
            match ipc::remove_agent(id).await {
                Ok(()) => {
                    if store.selected_agent.get_untracked() == Some(id) {
                        store.selected_agent.set(None);
                    }
                    store.refresh_project();
                }
                Err(e) => store.report(e),
            }
        });
    };

    view! {
        <li
            class="agent"
            class:selected=move || store.selected_agent.get() == Some(id)
            data-agent=id.to_string()
            title="Clic derecho para renombrar"
            on:click=move |_| store.select_agent(id, worktree, feature)
            on:contextmenu=move |ev| store.open_menu(&ev, MenuTarget::Agent(id))
        >
            <span class=format!("dot {class}") title=label.clone()></span>
            <Show
                when=renaming
                fallback={
                    let name = name.clone();
                    move || view! { <span class="agent-name ellipsis">{name.clone()}</span> }
                }
            >
                <InlineRename initial=name.clone() target=rename_target.clone() on_commit=commit/>
            </Show>
            <span class="agent-provider muted">{provider}</span>
            <span class="agent-status muted">{label}</span>
            <span class="actions">
                {if active {
                    view! { <button class="icon" title="Detener" on:click=stop>"■"</button> }.into_any()
                } else {
                    view! {
                        <button class="icon" title="Reiniciar" on:click=restart>"↻"</button>
                        <button class="icon danger" title="Quitar" on:click=remove>"×"</button>
                    }
                        .into_any()
                }}
            </span>
        </li>
    }
}

#[component]
fn NewAgent(feature: u64, worktrees: Vec<u64>) -> impl IntoView {
    let store = expect_context::<Store>();
    let first = worktrees.first().copied().unwrap_or_default();
    let (worktree, set_worktree) = signal(first);
    let providers = move || {
        store
            .project
            .with(|p| p.as_ref().map(|p| p.providers.clone()).unwrap_or_default())
    };
    let first_available = move || {
        providers()
            .into_iter()
            .find(|p| p.available)
            .map(|p| p.id)
            .unwrap_or_else(|| "claude".into())
    };
    let (provider, set_provider) = signal(first_available());
    let multiple = worktrees.len() > 1;
    let worktree_options = worktrees.clone();

    let launch = move |_| {
        let provider = provider.get_untracked();
        let wt = worktree.get_untracked();
        spawn_local(async move {
            match ipc::spawn_agent(feature, wt, provider).await {
                Ok(agent) => {
                    store.refresh_project();
                    store.select_agent(agent, wt, feature);
                }
                Err(e) => {
                    store.refresh_project();
                    store.report(e);
                }
            }
        });
    };

    view! {
        <div class="new-agent">
            <select class="provider" on:change=move |ev| set_provider.set(event_target_value(&ev))>
                {move || {
                    providers()
                        .into_iter()
                        .map(|p| {
                            let label = if p.available {
                                p.id.clone()
                            } else {
                                format!("{} (no instalado)", p.id)
                            };
                            let selected = p.id == provider.get_untracked();
                            view! {
                                <option value=p.id.clone() disabled=!p.available selected=selected>
                                    {label}
                                </option>
                            }
                        })
                        .collect_view()
                }}
            </select>
            <Show when=move || multiple>
                <select on:change=move |ev| {
                    set_worktree.set(event_target_value(&ev).parse().unwrap_or(first))
                }>
                    {worktree_options
                        .iter()
                        .map(|id| {
                            let label = store
                                .project
                                .with_untracked(|p| {
                                    p.as_ref()
                                        .and_then(|p| logic::worktree(p, *id))
                                        .map(logic::worktree_label)
                                })
                                .unwrap_or_default();
                            view! { <option value=id.to_string()>{label}</option> }
                        })
                        .collect_view()}
                </select>
            </Show>
            <button class="launch" on:click=launch>"Lanzar agente"</button>
        </div>
    }
}
