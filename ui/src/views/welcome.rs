//! Pantalla de bienvenida: abrir un repositorio en esta ventana.

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::ipc;
use crate::store::Store;

fn open(store: Store, path: String) {
    if path.trim().is_empty() {
        return;
    }
    spawn_local(async move {
        match ipc::open_project(path).await {
            Ok(res) if !res.other_window => store.refresh_project(),
            Ok(_) => {}
            Err(e) => store.report(e),
        }
    });
}

#[component]
pub fn Welcome() -> impl IntoView {
    let store = expect_context::<Store>();
    let (path, set_path) = signal(String::new());

    let pick = move |_| {
        spawn_local(async move {
            match ipc::pick_folder().await {
                Ok(Some(folder)) => open(store, folder),
                Ok(None) => {}
                Err(e) => store.report(e),
            }
        });
    };

    view! {
        <div class="welcome">
            <h1>"Gryphus"</h1>
            <p class="muted">"Una ventana por proyecto. Abre un repositorio git para empezar."</p>
            <button id="pick-folder" class="primary" on:click=pick>"Abrir carpeta…"</button>
            <form
                class="row"
                on:submit=move |ev: leptos::ev::SubmitEvent| {
                    ev.prevent_default();
                    open(store, path.get_untracked());
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
