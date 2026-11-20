//! Vista central: pestañas Agente / Editor / Diff. Las tres permanecen montadas
//! (se ocultan con CSS) para no perder la terminal ni el editor al cambiar.

use leptos::prelude::*;

use super::diff::DiffView;
use super::editor::CodeEditor;
use super::terminal::AgentTerminal;
use crate::layout::CenterView;
use crate::store::Store;

#[component]
pub fn MainView() -> impl IntoView {
    let store = expect_context::<Store>();
    let shown = move |v: CenterView| {
        move || {
            if store.view.get() == v {
                "flex"
            } else {
                "none"
            }
        }
    };

    view! {
        <main class="pane pane-center">
            <nav class="tabs" role="tablist">
                {CenterView::ALL
                    .iter()
                    .map(|&v| {
                        view! {
                            <button
                                class="tab"
                                role="tab"
                                id=format!("tab-{}", v.label().to_lowercase())
                                title=v.shortcut()
                                aria-selected=move || (store.view.get() == v).to_string()
                                on:click=move |_| store.view.set(v)
                            >
                                {v.label()}
                            </button>
                        }
                    })
                    .collect_view()}
            </nav>
            <div class="view" style:display=shown(CenterView::Agent)>
                <AgentTerminal/>
            </div>
            <div class="view" style:display=shown(CenterView::Editor)>
                <CodeEditor/>
            </div>
            <div class="view" style:display=shown(CenterView::Diff)>
                <DiffView/>
            </div>
        </main>
    }
}
