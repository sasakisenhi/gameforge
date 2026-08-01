//! Top-level desktop shell and navigation.

use crate::views::{
    brand_symbol, connection_presentation, development_board, inbox_page, placeholder,
};
use crate::{ColorTheme, CommandResult, Route, UiAction, UiState, reduce_ui_state};
use dioxus::prelude::*;
use gameforge_application::{AppShellView, ApplicationCommand};

const APP_CSS: &str = include_str!("app.css");

#[must_use]
pub fn render_app(view: &AppShellView) -> String {
    dioxus_ssr::render_element(rsx! { App { initial_view: view.clone() } })
}

#[must_use]
pub fn render_inbox(view: &AppShellView) -> String {
    dioxus_ssr::render_element(rsx! { InboxPreview { view: view.clone() } })
}

#[component]
fn InboxPreview(view: AppShellView) -> Element {
    inbox_page(&view, None, None, None)
}

#[component]
pub fn App(
    initial_view: AppShellView,
    on_command: Option<Callback<ApplicationCommand, CommandResult>>,
) -> Element {
    let app_view = use_signal(move || initial_view);
    let ui_state = use_signal(UiState::default);
    let command_notice = use_signal(|| None::<String>);
    let view = app_view.read().clone();
    let state = ui_state.read().clone();
    let connection = connection_presentation(&view.connection);
    let night_mode = state.color_theme == ColorTheme::Night;
    let shell_class = if night_mode {
        "app-shell theme-night"
    } else {
        "app-shell"
    };
    let mut theme_signal = ui_state;
    let nav_items = [
        (Route::Intent, "Intent", "01"),
        (Route::PlanReview, "Plan Review", "02"),
        (Route::Development, "Development", "03"),
        (Route::Inbox, "Inbox", "04"),
        (Route::BuildAcceptance, "Build & Acceptance", "05"),
    ]
    .into_iter()
    .map(|(route, label, index)| {
        let mut state_signal = ui_state;
        let active = state.route == route;
        rsx! {
            button {
                class: if active { "nav-item active" } else { "nav-item" },
                aria_current: if active { "page" } else { "false" },
                onclick: move |_| {
                    let next = reduce_ui_state(
                        state_signal.read().clone(),
                        &UiAction::Navigate(route),
                    );
                    state_signal.set(next);
                },
                span { class: "nav-index", "{index}" }
                span { "{label}" }
                if route == Route::Inbox && view.inbox_count > 0 {
                    span { class: "nav-badge", "{view.inbox_count}" }
                }
            }
        }
    });

    rsx! {
        style { {APP_CSS} }
        div { class: "{shell_class}",
            header { class: "topbar",
                div { class: "brand-block",
                    {brand_symbol()}
                    div { class: "brand-type",
                        span { class: "brand-name", "gameforge" }
                        span { class: "brand-tagline", "BUILD CONTROL" }
                    }
                }
                div { class: "project-context",
                    span { class: "context-label", "ACTIVE PROJECT" }
                    div { class: "context-project",
                        strong { "{view.project_name}" }
                        span { class: "project-separator", "/" }
                        span { class: "meta-path", "{view.project_root}" }
                    }
                    span { class: "meta-commit", "main · {view.main_commit}" }
                }
                div { class: "topbar-actions",
                    button {
                        class: "theme-toggle",
                        title: if night_mode { "Switch to light mode" } else { "Switch to night mode" },
                        aria_pressed: "{night_mode}",
                        onclick: move |_| {
                            let next = reduce_ui_state(
                                theme_signal.read().clone(),
                                &UiAction::ToggleTheme,
                            );
                            theme_signal.set(next);
                        },
                        span { class: "theme-icon", if night_mode { "☀" } else { "◐" } }
                        span { if night_mode { "DAY" } else { "NIGHT" } }
                    }
                    button { class: "command-trigger", title: "Command palette",
                        span { "COMMAND" }
                        kbd { "⌘ K" }
                    }
                    div { class: "connection {connection.class_name}",
                        span { class: "connection-dot" }
                        span { "{connection.label}" }
                    }
                }
            }

            if let Some(reason) = connection.disconnect_reason {
                div { class: "disconnect-banner", role: "alert",
                    strong { "読み取り専用" }
                    span { "{reason}" }
                    span { "再接続後、最新Projectionを確認して操作してください。" }
                }
            }

            div { class: "workspace",
                aside { class: "sidebar",
                    div { class: "sidebar-heading",
                        p { class: "sidebar-label", "WORKFLOW" }
                        span { "01—05" }
                    }
                    nav { {nav_items} }
                    div { class: "sidebar-footer",
                        p { "PROJECTION" }
                        div { class: "projection-line",
                            span { class: "projection-pulse" }
                            strong { "rev {view.projection_revision}" }
                        }
                        span { "同期: {view.last_synced_at}" }
                    }
                }

                main { class: "content",
                    if let Some(notice) = command_notice.read().as_ref() {
                        div { class: "command-notice", "{notice}" }
                    }
                    {
                        match state.route {
                            Route::Development => development_board(
                                &view,
                                &state,
                                ui_state,
                                command_notice,
                                app_view,
                                on_command,
                            ),
                            Route::Inbox => inbox_page(
                                &view,
                                Some(command_notice),
                                Some(app_view),
                                on_command,
                            ),
                            route => placeholder(route),
                        }
                    }
                }
            }

            footer { class: "statusbar",
                span { class: "status-brand", "GF / COORDINATOR" }
                span { "STATUS · {connection.label}" }
                span { "CAPACITY · {view.max_concurrent_task_runs}" }
                span { "PROJECTION · R{view.projection_revision}" }
            }
        }
    }
}
