/// Transfer timeline visualization component.
use dioxus::prelude::*;

/// Visual timeline of a transfer's lifecycle.
#[component]
pub fn TransferTimeline(
    status: String,
    created_at: String,
    completed_at: Option<String>,
) -> Element {
    let steps = vec![
        TimelineStep {
            label: "Lock Submitted".to_string(),
            description: "Transaction submitted on source chain".to_string(),
            time: created_at,
            completed: true,
        },
        TimelineStep {
            label: "Proof Generated".to_string(),
            description: "Cross-chain proof generated".to_string(),
            time: String::new(),
            completed: matches!(status.to_lowercase().as_str(), "in_progress" | "completed"),
        },
        TimelineStep {
            label: "Mint Completed".to_string(),
            description: "Sanad minted on destination chain".to_string(),
            time: completed_at.unwrap_or_default(),
            completed: status.to_lowercase() == "completed",
        },
    ];

    rsx! {
        div { class: "space-y-0",
            {steps.iter().enumerate().map(|(i, step)| {
                rsx! {
                    div { key: "{i}", class: "flex gap-4",
                        // Line
                        div { class: "flex flex-col items-center",
                            div {
                                class: "w-8 h-8 rounded-full flex items-center justify-center text-sm {step_icon_class(step.completed)}",
                                if step.completed {
                                    "✓"
                                } else {
                                    "○"
                                }
                            }
                            if i < steps.len() - 1 {
                                div { class: "w-0.5 h-12 {step_line_class(step.completed)}" }
                            }
                        }
                        // Content
                        div { class: "pb-8",
                            div { class: "font-medium {step_text_class(step.completed)}",
                                "{step.label}"
                            }
                            div { class: "text-sm text-gray-500",
                                "{step.description}"
                            }
                            if !step.time.is_empty() {
                                div { class: "text-xs text-gray-600 mt-1",
                                    "{step.time}"
                                }
                            }
                        }
                    }
                }
            })}
        }
    }
}

struct TimelineStep {
    label: String,
    description: String,
    time: String,
    completed: bool,
}

fn step_icon_class(completed: bool) -> &'static str {
    if completed {
        "bg-green-500/20 text-green-400 border border-green-500/30"
    } else {
        "bg-gray-800 text-gray-600 border border-gray-700"
    }
}

fn step_line_class(completed: bool) -> &'static str {
    if completed {
        "bg-green-500/30"
    } else {
        "bg-gray-800"
    }
}

fn step_text_class(completed: bool) -> &'static str {
    if completed {
        "text-gray-200"
    } else {
        "text-gray-500"
    }
}
