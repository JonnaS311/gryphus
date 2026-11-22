//! Vista central: Agente / Editor / Diff repartidas en columnas (`CenterLayout`).
//! Las tres permanecen montadas (no se pierden la terminal ni el editor): cada una
//! se coloca en la columna de su grid y se oculta con CSS si no es la activa.
//!
//! Arrastrar una pestaña muestra zonas de soltar sobre cada columna: los bordes
//! izquierdo/derecho la abren en una columna nueva a ese lado; la barra superior o
//! el centro la juntan como pestaña de esa columna (así se vuelve al estado inicial).

use leptos::prelude::*;

use super::diff::DiffView;
use super::editor::CodeEditor;
use super::terminal::AgentTerminal;
use crate::layout::{CenterView, DropTarget};
use crate::store::Store;

/// Píxeles que hay que mover el puntero para que un clic en una pestaña pase a arrastre.
const DRAG_THRESHOLD: i32 = 6;

#[derive(Debug, Clone, Copy)]
struct Press {
    view: CenterView,
    x: i32,
    y: i32,
}

#[component]
pub fn MainView() -> impl IntoView {
    let store = expect_context::<Store>();
    let press = StoredValue::new(None::<Press>);
    let dragging = RwSignal::new(None::<CenterView>);
    let target = RwSignal::new(None::<DropTarget>);
    let columns = Memo::new(move |_| store.center.with(|c| c.columns.len()));

    let reset = move || {
        press.set_value(None);
        if dragging.get_untracked().is_some() {
            dragging.set(None);
        }
        if target.get_untracked().is_some() {
            target.set(None);
        }
    };
    let on_move = window_event_listener(leptos::ev::pointermove, move |ev| {
        let Some(p) = press.try_get_value().flatten() else {
            return;
        };
        // Se soltó el botón fuera de la ventana: no llegó `pointerup`.
        if ev.buttons() & 1 == 0 {
            reset();
            return;
        }
        if dragging.get_untracked().is_none()
            && ((ev.client_x() - p.x).abs() > DRAG_THRESHOLD
                || (ev.client_y() - p.y).abs() > DRAG_THRESHOLD)
        {
            dragging.set(Some(p.view));
        }
    });
    let on_up = window_event_listener(leptos::ev::pointerup, move |_| {
        if press.try_get_value().flatten().is_none() {
            return;
        }
        if let (Some(view), Some(drop)) = (dragging.get_untracked(), target.get_untracked()) {
            store.move_view(view, drop);
        }
        reset();
    });
    let on_cancel = window_event_listener(leptos::ev::pointercancel, move |_| {
        if press.try_get_value().flatten().is_some() {
            reset();
        }
    });
    on_cleanup(move || {
        on_move.remove();
        on_up.remove();
        on_cancel.remove();
    });

    // Columna y visibilidad de cada vista; `Memo` para no tocar el DOM si no cambian.
    let cell = move |v: CenterView| {
        let column = Memo::new(move |_| store.center.with(|c| c.column_of(v)));
        let shown = Memo::new(move |_| store.center.with(|c| c.is_visible(v)));
        view! {
            <div
                class="view"
                class:split={move || column.get() > 0}
                style:grid-column=move || (column.get() + 1).to_string()
                style:display=move || if shown.get() { "flex" } else { "none" }
                // Tocar una vista le da el foco (atajos como Ctrl+W del editor).
                on:pointerdown=move |_| {
                    if store.view.get_untracked() != v {
                        store.view.set(v);
                    }
                }
            >
                {match v {
                    CenterView::Agent => view! { <AgentTerminal/> }.into_any(),
                    CenterView::Editor => view! { <CodeEditor/> }.into_any(),
                    CenterView::Diff => view! { <DiffView/> }.into_any(),
                }}
            </div>
        }
    };

    view! {
        <main
            class="pane pane-center"
            class:dragging=move || dragging.with(Option::is_some)
            style:grid-template-columns=move || {
                format!("repeat({}, minmax(0, 1fr))", columns.get())
            }
        >
            {move || {
                store
                    .center
                    .with(|c| c.columns.clone())
                    .into_iter()
                    .enumerate()
                    .map(|(i, col)| {
                        view! {
                            <nav
                                class="tabs"
                                class:split={i > 0}
                                role="tablist"
                                style:grid-column=(i + 1).to_string()
                            >
                                {col
                                    .tabs
                                    .into_iter()
                                    .map(|v| {
                                        let active = v == col.active;
                                        view! {
                                            <button
                                                class="tab"
                                                class:focused=move || store.view.get() == v
                                                class:lifted=move || dragging.get() == Some(v)
                                                role="tab"
                                                id=format!("tab-{}", v.label().to_lowercase())
                                                title=format!(
                                                    "{} · arrastra a un lado para abrirla junto a las demás",
                                                    v.shortcut(),
                                                )
                                                aria-selected=active.to_string()
                                                on:pointerdown=move |ev| {
                                                    if ev.button() == 0 {
                                                        press.set_value(Some(Press {
                                                            view: v,
                                                            x: ev.client_x(),
                                                            y: ev.client_y(),
                                                        }));
                                                    }
                                                }
                                                on:click=move |_| store.show_view(v)
                                            >
                                                {v.label()}
                                            </button>
                                        }
                                    })
                                    .collect_view()}
                            </nav>
                        }
                    })
                    .collect_view()
            }}
            {cell(CenterView::Agent)}
            {cell(CenterView::Editor)}
            {cell(CenterView::Diff)}
            {move || {
                dragging
                    .get()
                    .map(|_| {
                        (0..columns.get())
                            .map(|i| view! { <DropColumn column=i target/> })
                            .collect_view()
                    })
            }}
        </main>
    }
}

/// Zonas de soltar sobre una columna mientras se arrastra una pestaña.
#[component]
fn DropColumn(column: usize, target: RwSignal<Option<DropTarget>>) -> impl IntoView {
    let into = DropTarget::Into(column);
    let left = DropTarget::Split {
        column,
        after: false,
    };
    let right = DropTarget::Split {
        column,
        after: true,
    };
    let zone = move |class: &'static str, drop: DropTarget| {
        view! {
            <div class=format!("drop-zone {class}") on:pointerenter=move |_| target.set(Some(drop))></div>
        }
    };
    view! {
        <div
            class="drop-column"
            style:grid-column=(column + 1).to_string()
            data-drop=move || match target.get() {
                Some(t) if t == into => "into",
                Some(t) if t == left => "left",
                Some(t) if t == right => "right",
                _ => "",
            }
            on:pointerleave=move |_| {
                if target.get_untracked().is_some_and(|t| t == into || t == left || t == right) {
                    target.set(None);
                }
            }
        >
            {zone("top", into)}
            {zone("left", left)}
            {zone("middle", into)}
            {zone("right", right)}
        </div>
    }
}
