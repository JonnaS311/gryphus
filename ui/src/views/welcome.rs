//! Pantalla de bienvenida: abrir un repositorio en esta ventana.

use leptos::prelude::*;

use crate::store::Store;

#[component]
pub fn Welcome() -> impl IntoView {
    let store = expect_context::<Store>();
    let (path, set_path) = signal(String::new());

    view! {
        <div class="welcome">
            <h1>"Gryphus"</h1>
            <p class="muted">"Una ventana por proyecto. Abre un repositorio git para empezar."</p>
            <button id="pick-folder" class="primary" on:click=move |_| store.pick_and_open()>
                "Abrir carpeta…"
            </button>
            <form
                class="row"
                on:submit=move |ev: leptos::ev::SubmitEvent| {
                    ev.prevent_default();
                    store.open_project(path.get_untracked());
                }
            >
                <input
                    id="project-path"
                    placeholder="o escribe la ruta del repositorio"
                    prop:value=path
                    on:input=move |ev| set_path.set(event_target_value(&ev))
                />
                <button id="open-path" type="submit">"Abrir"</button>
            </form>
        </div>
    }
}
