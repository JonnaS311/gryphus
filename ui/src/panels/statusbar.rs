//! Barra de estado: branch del worktree activo, ahead/behind, cambios y agentes.

use leptos::prelude::*;

use crate::logic;
use crate::store::Store;

#[component]
pub fn StatusBar() -> impl IntoView {
    let store = expect_context::<Store>();
    let branch = move || {
        store.project.with(|p| {
            p.as_ref()
                .zip(store.active_worktree.get())
                .and_then(|(p, id)| logic::worktree(p, id))
                .and_then(|w| w.branch.clone())
        })
    };
    let tracking = move || {
        let name = branch()?;
        store.overview.with(|o| {
            o.as_ref()?
                .branches
                .iter()
                .find(|b| !b.is_remote && b.name == name)
                .map(logic::tracking_text)
        })
    };
    let running = move || {
        store.project.with(|p| {
            p.as_ref()
                .map(|p| p.agents.iter().filter(|a| a.status.is_active()).count())
                .unwrap_or(0)
        })
    };
    let root = move || store.project.with(|p| p.as_ref().map(|p| p.root.clone()));

    view! {
        <footer class="statusbar">
            <span id="status-branch">
                "⎇ " {move || branch().unwrap_or_else(|| "HEAD desacoplado".into())}
            </span>
            <span>{tracking}</span>
            <span id="status-changes">
                {move || {
                    let n = store.status.with(Vec::len);
                    if n == 1 { "1 cambio".to_owned() } else { format!("{n} cambios") }
                }}
            </span>
            <span>{move || format!("{} agentes activos", running())}</span>
            <span class="spacer"></span>
            <span class="muted ellipsis">{root}</span>
        </footer>
    }
}
