//! Barra superior: abrir y cerrar proyecto, y configuración de apariencia.

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::settings::{clamp_size, Settings, MONO_FONTS, SIZE_MAX, SIZE_MIN, UI_FONTS};
use crate::store::Store;

#[component]
pub fn Navbar() -> impl IntoView {
    let store = expect_context::<Store>();
    let has_project = move || store.project.with(Option::is_some);
    let project_name = move || {
        store
            .project
            .with(|p| p.as_ref().map(|p| p.name.clone()).unwrap_or_default())
    };
    let project_root = move || {
        store
            .project
            .with(|p| p.as_ref().map(|p| p.root.clone()).unwrap_or_default())
    };

    let close = move |_| {
        let warning = store.close_warning();
        spawn_local(async move {
            if let Some(warning) = warning {
                if !store.ask(warning).await {
                    return;
                }
            }
            store.close_project();
        });
    };

    view! {
        <nav class="navbar">
            <span class="brand">"Gryphus"</span>
            <button
                id="nav-open"
                class="nav-btn"
                title="Abrir un repositorio (en otra ventana si esta ya tiene uno)"
                on:click=move |_| store.pick_and_open()
            >
                "Abrir proyecto…"
            </button>
            <button
                id="nav-close"
                class="nav-btn"
                title="Cerrar el proyecto de esta ventana"
                disabled=move || !has_project()
                on:click=close
            >
                "Cerrar proyecto"
            </button>
            <span class="nav-project ellipsis" title=project_root>
                {project_name}
            </span>
            <button
                id="nav-settings"
                class="nav-btn"
                class:on=move || store.settings_open.get()
                on:click=move |_| store.settings_open.update(|o| *o = !*o)
            >
                "⚙ Configuración"
            </button>
        </nav>
        <Show when=move || store.settings_open.get()>
            <SettingsDialog/>
        </Show>
    }
}

#[component]
fn SettingsDialog() -> impl IntoView {
    let store = expect_context::<Store>();
    let edit = move |f: &dyn Fn(&mut Settings)| {
        let mut s = store.settings.get_untracked();
        f(&mut s);
        store.set_settings(s);
    };
    let size_input = move |value: String, set: fn(&mut Settings, u8)| {
        if let Ok(n) = value.parse::<u8>() {
            edit(&|s| set(s, clamp_size(n)));
        }
    };

    let close = move || store.settings_open.set(false);
    let _esc = window_event_listener(leptos::ev::keydown, move |ev| {
        if ev.key() == "Escape" && store.confirm.with_untracked(Option::is_none) {
            close();
        }
    });

    view! {
        <div class="backdrop" on:click=move |_| close()></div>
        <section class="settings" role="dialog" aria-label="Configuración">
            <header class="settings-header">
                <h2>"Configuración"</h2>
                <button class="icon" title="Cerrar (Esc)" on:click=move |_| close()>"✕"</button>
            </header>

            <h3>"Interfaz"</h3>
            <label>
                "Tipografía"
                <select
                    id="set-ui-font"
                    on:change=move |ev| {
                        let v = event_target_value(&ev);
                        edit(&|s| s.ui_font = v.clone());
                    }
                >
                    {UI_FONTS
                        .iter()
                        .map(|(label, _)| {
                            view! {
                                <option
                                    value=*label
                                    selected=move || store.settings.with(|s| s.ui_font == *label)
                                >
                                    {*label}
                                </option>
                            }
                        })
                        .collect_view()}
                </select>
            </label>
            <label>
                "Tamaño de letra (px)"
                <input
                    id="set-ui-size"
                    type="number"
                    min=SIZE_MIN
                    max=SIZE_MAX
                    prop:value=move || store.settings.with(|s| s.ui_size.to_string())
                    on:change=move |ev| size_input(event_target_value(&ev), |s, n| s.ui_size = n)
                />
            </label>

            <h3>"Editor, terminal y diff"</h3>
            <label>
                "Tipografía"
                <select
                    id="set-mono-font"
                    on:change=move |ev| {
                        let v = event_target_value(&ev);
                        edit(&|s| s.mono_font = v.clone());
                    }
                >
                    {MONO_FONTS
                        .iter()
                        .map(|(label, _)| {
                            view! {
                                <option
                                    value=*label
                                    selected=move || store.settings.with(|s| s.mono_font == *label)
                                >
                                    {*label}
                                </option>
                            }
                        })
                        .collect_view()}
                </select>
            </label>
            <label>
                "Tamaño de letra (px)"
                <input
                    id="set-mono-size"
                    type="number"
                    min=SIZE_MIN
                    max=SIZE_MAX
                    prop:value=move || store.settings.with(|s| s.mono_size.to_string())
                    on:change=move |ev| size_input(event_target_value(&ev), |s, n| s.mono_size = n)
                />
            </label>

            <footer class="settings-footer">
                <span class="muted">"Las fuentes que no estén instaladas usan la siguiente disponible."</span>
                <button on:click=move |_| store.set_settings(Settings::default())>
                    "Restablecer"
                </button>
            </footer>
        </section>
    }
}
