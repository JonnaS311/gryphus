//! Menú contextual (clic derecho) y edición de nombres en línea, compartidos por
//! el panel de features y el explorador.

use leptos::prelude::*;

use crate::logic;
use crate::store::{Clipboard, MenuTarget, RenameTarget, Store};

const MENU_WIDTH: i32 = 200;
const ITEM_HEIGHT: i32 = 28;
/// Fila de muestras de color (`.menu-swatches` en style.css).
const SWATCHES_HEIGHT: i32 = 34;

struct MenuItem {
    label: &'static str,
    enabled: bool,
    action: Box<dyn Fn()>,
}

fn items(store: Store, target: MenuTarget) -> Vec<MenuItem> {
    let has_clip = store.clipboard.with_untracked(Option::is_some);
    match target {
        MenuTarget::Agent(id) => vec![MenuItem {
            label: "Renombrar",
            enabled: true,
            action: Box::new(move || store.renaming.set(Some(RenameTarget::Agent(id)))),
        }],
        // Las muestras de color las pinta `ColorSwatches`; aquí solo volver al automático.
        MenuTarget::Feature(id) => vec![MenuItem {
            label: "Color automático",
            enabled: store.feature_colors.with_untracked(|c| c.contains_key(&id)),
            action: Box::new(move || store.set_feature_color(id, None)),
        }],
        MenuTarget::Entry {
            worktree,
            path,
            is_dir,
        } => {
            let paste_dir = if is_dir {
                path.clone()
            } else {
                logic::parent_dir(&path).to_owned()
            };
            let (rename_path, copy_path) = (path.clone(), path);
            vec![
                MenuItem {
                    label: "Renombrar",
                    enabled: true,
                    action: Box::new(move || {
                        store.renaming.set(Some(RenameTarget::Entry {
                            worktree,
                            path: rename_path.clone(),
                        }))
                    }),
                },
                MenuItem {
                    label: "Copiar",
                    enabled: true,
                    action: Box::new(move || {
                        store.clipboard.set(Some(Clipboard {
                            worktree,
                            path: copy_path.clone(),
                        }))
                    }),
                },
                MenuItem {
                    label: "Pegar",
                    enabled: has_clip,
                    action: Box::new(move || store.paste(worktree, paste_dir.clone())),
                },
            ]
        }
        MenuTarget::ExplorerRoot { worktree } => vec![MenuItem {
            label: "Pegar",
            enabled: has_clip,
            action: Box::new(move || store.paste(worktree, String::new())),
        }],
    }
}

/// Se monta una vez en la raíz; muestra `Store::menu` cuando hay uno abierto.
#[component]
pub fn ContextMenuHost() -> impl IntoView {
    let store = expect_context::<Store>();
    let close = move || {
        if store.menu.with_untracked(Option::is_some) {
            store.menu.set(None);
        }
    };
    let _click = window_event_listener(leptos::ev::click, move |_| close());
    let _context = window_event_listener(leptos::ev::contextmenu, move |_| close());
    let _blur = window_event_listener(leptos::ev::blur, move |_| close());
    let _resize = window_event_listener(leptos::ev::resize, move |_| close());
    let _keys = window_event_listener(leptos::ev::keydown, move |ev| {
        if ev.key() == "Escape" && store.menu.with_untracked(Option::is_some) {
            ev.stop_propagation();
            close();
        }
    });

    move || {
        store.menu.get().map(|menu| {
            let feature = match menu.target {
                MenuTarget::Feature(id) => Some(id),
                _ => None,
            };
            let items = items(store, menu.target);
            let (width, height) = crate::bridge::prefs::viewport();
            let swatches_height = if feature.is_some() {
                SWATCHES_HEIGHT
            } else {
                0
            };
            let menu_height = items.len() as i32 * ITEM_HEIGHT + swatches_height + 10;
            let x = menu.x.min(width - MENU_WIDTH - 4).max(0);
            let y = menu.y.min(height - menu_height - 4).max(0);
            view! {
                <ul
                    class="context-menu"
                    role="menu"
                    style:left=format!("{x}px")
                    style:top=format!("{y}px")
                    on:contextmenu=|ev| ev.prevent_default()
                >
                    {feature.map(|id| view! { <ColorSwatches feature=id/> })}
                    {items
                        .into_iter()
                        .map(|item| {
                            let MenuItem { label, enabled, action } = item;
                            view! {
                                <li>
                                    <button
                                        role="menuitem"
                                        data-action=label
                                        disabled=!enabled
                                        on:click=move |ev| {
                                            ev.stop_propagation();
                                            store.menu.set(None);
                                            action();
                                        }
                                    >
                                        {label}
                                    </button>
                                </li>
                            }
                        })
                        .collect_view()}
                </ul>
            }
        })
    }
}

/// Selector limitado a la paleta de las features (`logic::FEATURE_HUES`); marca el
/// color que tiene ahora, elegido o automático.
#[component]
fn ColorSwatches(feature: u64) -> impl IntoView {
    let store = expect_context::<Store>();
    let current = store
        .feature_colors
        .with_untracked(|c| c.get(&feature).copied());
    let automatic = store.project.with_untracked(|p| {
        p.as_ref()
            .and_then(|p| p.features.iter().position(|f| f.id == feature))
            .map(logic::feature_hue)
    });
    let shown = current.or(automatic);
    view! {
        <li class="menu-swatches" role="group" aria-label="Color de la feature">
            {logic::FEATURE_HUES
                .iter()
                .map(|&(hue, name)| {
                    let selected = shown == Some(hue);
                    view! {
                        <button
                            class="swatch"
                            class:selected=selected
                            role="menuitemradio"
                            aria-checked=selected.to_string()
                            aria-label=name
                            title=name
                            data-hue=hue.to_string()
                            style=format!("--feature-hue: {hue}")
                            on:click=move |ev| {
                                ev.stop_propagation();
                                store.menu.set(None);
                                store.set_feature_color(feature, Some(hue));
                            }
                        ></button>
                    }
                })
                .collect_view()}
        </li>
    }
}

/// Campo para renombrar en línea. Enter o perder el foco confirma; Escape cancela.
/// `on_commit` solo se llama si el nombre cambió y no está vacío.
#[component]
pub fn InlineRename(
    initial: String,
    target: RenameTarget,
    /// Archivo: se preselecciona el nombre sin la extensión.
    #[prop(optional)]
    is_file: bool,
    #[prop(into)] on_commit: Callback<String>,
) -> impl IntoView {
    let store = expect_context::<Store>();
    let input = NodeRef::<leptos::html::Input>::new();
    let select_end = logic::rename_selection(&initial, is_file);
    input.on_load(move |el| {
        request_animation_frame(move || {
            let _ = el.focus();
            let _ = el.set_selection_range(0, select_end);
        });
    });
    let (initial_v, target_v) = (StoredValue::new(initial.clone()), StoredValue::new(target));

    let commit = move || {
        if !store.finish_renaming(&target_v.get_value()) {
            return;
        }
        let value = input
            .get_untracked()
            .map(|el| el.value().trim().to_owned())
            .unwrap_or_default();
        if !value.is_empty() && initial_v.with_value(|i| *i != value) {
            on_commit.run(value);
        }
    };

    view! {
        <input
            class="rename-input"
            node_ref=input
            prop:value=initial
            spellcheck="false"
            on:click=|ev| ev.stop_propagation()
            on:dblclick=|ev| ev.stop_propagation()
            on:contextmenu=|ev| ev.stop_propagation()
            on:keydown=move |ev| {
                ev.stop_propagation();
                match ev.key().as_str() {
                    "Enter" => {
                        ev.prevent_default();
                        commit();
                    }
                    "Escape" => {
                        ev.prevent_default();
                        store.finish_renaming(&target_v.get_value());
                    }
                    _ => {}
                }
            }
            on:blur=move |_| commit()
        />
    }
}
