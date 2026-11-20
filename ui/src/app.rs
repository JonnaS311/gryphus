//! Shell de la aplicación: tres paneles + barra de estado.
//! En el hito 0 los paneles están vacíos; se llenan en el hito 8.

use leptos::prelude::*;

use crate::layout::CenterView;

#[component]
pub fn App() -> impl IntoView {
    let (active, set_active) = signal(CenterView::Agent);

    view! {
        <div class="shell">
            <aside class="pane pane-left">
                <h2 class="pane-title">"Features"</h2>
            </aside>

            <main class="pane pane-center">
                <nav class="tabs" role="tablist">
                    {CenterView::ALL
                        .iter()
                        .map(|&view| {
                            view! {
                                <button
                                    class="tab"
                                    role="tab"
                                    title=view.shortcut()
                                    aria-selected=move || (active.get() == view).to_string()
                                    on:click=move |_| set_active.set(view)
                                >
                                    {view.label()}
                                </button>
                            }
                        })
                        .collect_view()}
                </nav>
                <p class="empty">"Abre un proyecto para empezar"</p>
            </main>

            <aside class="pane pane-right">
                <h2 class="pane-title">"Explorador"</h2>
            </aside>

            <footer class="statusbar">
                <span>"Gryphus v" {env!("CARGO_PKG_VERSION")}</span>
            </footer>
        </div>
    }
}
