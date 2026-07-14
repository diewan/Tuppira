/// Reusable dropdown select component.
use dioxus::prelude::*;

/// Dropdown option button component.
#[component]
fn DropdownOption<T: PartialEq + Clone + std::fmt::Display + 'static>(
    option: T,
    selected_str: String,
    idx: usize,
    on_change: EventHandler<T>,
    on_close: EventHandler<()>,
) -> Element {
    let is_selected = option.to_string() == selected_str;

    rsx! {
        button {
            key: "{idx}",
            onclick: move |_: MouseEvent| {
                on_change.call(option.clone());
                on_close.call(());
            },
            class: "w-full px-4 py-3 text-left text-gray-100 hover:bg-gray-700 transition-colors first:rounded-t-lg last:rounded-b-lg",
            class: if is_selected { "bg-blue-600/20 text-blue-400" } else { "" },
            "{option}"
        }
    }
}

/// Generic dropdown select component for types that implement Display.
#[component]
pub fn Dropdown<T: PartialEq + Clone + std::fmt::Display + 'static>(
    options: Vec<T>,
    selected: T,
    on_change: EventHandler<T>,
) -> Element {
    let mut is_open = use_signal(|| false);
    let selected_str = selected.to_string();

    rsx! {
        div { class: "relative",
            // Selected value button
            button {
                onclick: move |_| is_open.toggle(),
                class: "w-full bg-gray-800 border border-gray-700 rounded-lg px-4 py-3 text-left text-gray-100 hover:border-gray-600 focus:outline-none focus:ring-2 focus:ring-blue-500 flex items-center justify-between",
                span { "{selected_str}" }
                span { class: "text-gray-400",
                    if is_open() { "▲" } else { "▼" }
                }
            }

            // Dropdown menu
            if is_open() {
                div { class: "absolute z-50 w-full mt-2 bg-gray-800 border border-gray-700 rounded-lg shadow-xl max-h-60 overflow-y-auto",
                    for (idx, option) in options.into_iter().enumerate() {
                        DropdownOption {
                            key: "{idx}",
                            option,
                            selected_str: selected_str.clone(),
                            idx,
                            on_change: on_change.clone(),
                            on_close: move |_| is_open.set(false),
                        }
                    }
                }
            }

            // Backdrop to close dropdown
            if is_open() {
                div {
                    class: "fixed inset-0 z-40",
                    onclick: move |_| is_open.set(false),
                }
            }
        }
    }
}
