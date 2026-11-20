//! Una sola instancia de xterm por ventana. Al cambiar de agente se reinicia y
//! se vuelve a conectar; el scrollback lo envía el backend.

use leptos::html;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::bridge::xterm::Terminal;
use crate::ipc;
use crate::logic;
use crate::store::Store;

#[component]
pub fn AgentTerminal() -> impl IntoView {
    let store = expect_context::<Store>();
    let el = NodeRef::<html::Div>::new();
    let term = StoredValue::new_local(None::<Terminal>);

    // Crea la terminal en cuanto el contenedor existe.
    Effect::new(move |_| {
        let Some(div) = el.get() else { return };
        if term.with_value(Option::is_some) {
            return;
        }
        let terminal = Terminal::new(
            &div,
            move |data: String| {
                if let Some(agent) = store.selected_agent.get_untracked() {
                    spawn_local(async move {
                        let _ = ipc::agent_input(agent, data).await;
                    });
                }
            },
            move |cols, rows| {
                if let Some(agent) = store.selected_agent.get_untracked() {
                    spawn_local(async move {
                        let _ = ipc::agent_resize(agent, cols, rows).await;
                    });
                }
            },
        );
        term.set_value(Some(terminal));
    });

    // Conecta la terminal al agente seleccionado.
    Effect::new(move |_| {
        let agent = store.selected_agent.get();
        let _ = el.get();
        term.with_value(|t| {
            let Some(t) = t else { return };
            t.reset();
            match agent {
                Some(agent) => {
                    let channel = t.channel();
                    spawn_local(async move {
                        if let Err(e) = ipc::attach_agent(agent, &channel).await {
                            store.report(e);
                        }
                    });
                    t.fit();
                    t.focus();
                }
                None => {
                    spawn_local(async {
                        let _ = ipc::detach_agent().await;
                    });
                    t.write(
                        "\x1b[90mSelecciona o lanza un agente en el panel izquierdo.\x1b[0m\r\n",
                    );
                }
            }
        });
    });

    on_cleanup(move || {
        term.update_value(|t| {
            if let Some(t) = t.take() {
                t.dispose();
            }
        })
    });

    let title = move || {
        store.project.with(|p| {
            let p = p.as_ref()?;
            let agent = p
                .agents
                .iter()
                .find(|a| Some(a.id) == store.selected_agent.get())?;
            let (class, label) = logic::status_badge(&agent.status);
            Some((format!("{} #{}", agent.provider, agent.id), class, label))
        })
    };

    view! {
        <div class="view-header">
            {move || {
                title()
                    .map(|(name, class, label)| {
                        view! {
                            <span class=format!("dot {class}")></span>
                            <span>{name}</span>
                            <span class="muted">{label}</span>
                        }
                    })
            }}
        </div>
        <div class="terminal" node_ref=el></div>
    }
}
