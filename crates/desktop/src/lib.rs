//! Dioxus desktop client and pure UI state transitions.

use dioxus::prelude::*;
use gameforgo_application::{
    AppShellView, ApplicationCommand, BoardIntent, ConnectionState, TaskRowView,
    command_for_board_intent,
};

const APP_CSS: &str = include_str!("app.css");

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Route {
    Intent,
    PlanReview,
    #[default]
    Development,
    Inbox,
    BuildAcceptance,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TaskFilter {
    #[default]
    All,
    Runnable,
    Running,
    Failed,
    HumanWait,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UiState {
    pub route: Route,
    pub task_filter: TaskFilter,
    pub selected_task_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiAction {
    Navigate(Route),
    SetTaskFilter(TaskFilter),
    SelectTask(Option<String>),
}

#[must_use]
pub fn reduce_ui_state(mut state: UiState, action: &UiAction) -> UiState {
    match action {
        UiAction::Navigate(route) => state.route = *route,
        UiAction::SetTaskFilter(filter) => state.task_filter = *filter,
        UiAction::SelectTask(task_id) => state.selected_task_id.clone_from(task_id),
    }
    state
}

#[must_use]
pub fn render_app(view: &AppShellView) -> String {
    dioxus_ssr::render_element(rsx! { App { initial_view: view.clone() } })
}

#[component]
pub fn App(initial_view: AppShellView) -> Element {
    let ui_state = use_signal(UiState::default);
    let command_notice = use_signal(|| None::<String>);
    let state = ui_state.read().clone();
    let connection = connection_presentation(&initial_view.connection);
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
                onclick: move |_| {
                    let next = reduce_ui_state(
                        state_signal.read().clone(),
                        &UiAction::Navigate(route),
                    );
                    state_signal.set(next);
                },
                span { class: "nav-index", "{index}" }
                span { "{label}" }
                if route == Route::Inbox && initial_view.inbox_count > 0 {
                    span { class: "nav-badge", "{initial_view.inbox_count}" }
                }
            }
        }
    });

    rsx! {
        style { {APP_CSS} }
        div { class: "app-shell",
            header { class: "topbar",
                div { class: "brand-block",
                    div { class: "brand-mark", "GF" }
                    div {
                        p { class: "eyebrow", "PROJECT COORDINATOR" }
                        h1 { "{initial_view.project_name}" }
                    }
                }
                div { class: "project-meta",
                    span { class: "meta-path", "{initial_view.project_root}" }
                    span { class: "meta-commit", "main @ {initial_view.main_commit}" }
                }
                div { class: "connection {connection.class_name}",
                    span { class: "connection-dot" }
                    span { "{connection.label}" }
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
                    p { class: "sidebar-label", "WORKFLOW" }
                    nav { {nav_items} }
                    div { class: "sidebar-footer",
                        p { "PROJECTION" }
                        strong { "rev {initial_view.projection_revision}" }
                        span { "同期: {initial_view.last_synced_at}" }
                    }
                }

                main { class: "content",
                    if let Some(notice) = command_notice.read().as_ref() {
                        div { class: "command-notice", "{notice}" }
                    }
                    {
                        match state.route {
                            Route::Development => development_board(
                                &initial_view,
                                &state,
                                ui_state,
                                command_notice,
                            ),
                            route => placeholder(route),
                        }
                    }
                }
            }

            footer { class: "statusbar",
                span { "Coordinator / {connection.label}" }
                span { "同時実行上限 {initial_view.max_concurrent_task_runs}" }
                span { "Projection rev {initial_view.projection_revision}" }
            }
        }
    }
}

#[allow(clippy::too_many_lines)]
fn development_board(
    view: &AppShellView,
    state: &UiState,
    ui_state: Signal<UiState>,
    command_notice: Signal<Option<String>>,
) -> Element {
    let summary = view.development.summary;
    let filters = [
        (TaskFilter::All, "All"),
        (TaskFilter::Runnable, "Runnable"),
        (TaskFilter::Running, "Running"),
        (TaskFilter::Failed, "Failed"),
        (TaskFilter::HumanWait, "Human wait"),
    ]
    .into_iter()
    .map(|(filter, label)| {
        let mut state_signal = ui_state;
        let active = state.task_filter == filter;
        rsx! {
            button {
                class: if active { "filter active" } else { "filter" },
                onclick: move |_| {
                    let next = reduce_ui_state(
                        state_signal.read().clone(),
                        &UiAction::SetTaskFilter(filter),
                    );
                    state_signal.set(next);
                },
                "{label}"
            }
        }
    });
    let visible_rows = view
        .development
        .task_rows
        .iter()
        .filter(|row| task_matches_filter(row, state.task_filter))
        .cloned()
        .collect::<Vec<_>>();
    let has_rows = !visible_rows.is_empty();
    let rows = visible_rows.into_iter().map(|row| {
        let selected = state.selected_task_id.as_deref() == Some(row.task_id.as_str());
        let selection_id = row.task_id.clone();
        let queue_id = row.task_id.clone();
        let command_view = view.clone();
        let mut state_signal = ui_state;
        let mut notice_signal = command_notice;
        let unavailable_reason = row.queue_unavailable_reason.clone().unwrap_or_default();
        let health_label = row.health_flags.join(", ");
        rsx! {
            tr {
                key: "{row.task_id}",
                class: if selected { "selected" } else { "" },
                onclick: move |_| {
                    let next = reduce_ui_state(
                        state_signal.read().clone(),
                        &UiAction::SelectTask(Some(selection_id.clone())),
                    );
                    state_signal.set(next);
                },
                td { class: "task-identity",
                    strong { "{row.task_id}" }
                    span { "{row.title}" }
                }
                td { {status_pill(&row.task_status, "task")} }
                td {
                    if let Some(run_status) = row.run_status.as_deref() {
                        {status_pill(run_status, "run")}
                    } else {
                        span { class: "muted", "—" }
                    }
                }
                td {
                    if row.health_flags.is_empty() {
                        span { class: "health-ok", "HEALTHY" }
                    } else {
                        span { class: "health-warning", "{health_label}" }
                    }
                }
                td { class: "actions",
                    button {
                        class: "queue-button",
                        disabled: !row.can_queue,
                        title: "{unavailable_reason}",
                        onclick: move |_| {
                            let notice = match command_for_board_intent(
                                &command_view,
                                BoardIntent::QueueTask { task_id: queue_id.clone() },
                            ) {
                                Ok(command) => command_description(&command),
                                Err(error) => format!("操作できません: {error}"),
                            };
                            notice_signal.set(Some(notice));
                        },
                        "Queue"
                    }
                }
            }
        }
    });

    rsx! {
        section { class: "page development-page",
            div { class: "page-heading",
                div {
                    p { class: "eyebrow", "WORK / EXECUTION" }
                    h2 { "Development" }
                    p { class: "page-description",
                        "TaskとTask Runの現在地を、再構築可能なProjectionから確認します。"
                    }
                }
                div { class: "revision-stamp", "VIEW REV {view.development.projection_revision}" }
            }

            div { class: "summary-grid",
                {summary_card("RUNNING", summary.running, "blue")}
                {summary_card("RUNNABLE", summary.runnable, "green")}
                {summary_card("QUEUED", summary.queued, "slate")}
                {summary_card("DEPENDENCY", summary.dependency_blocked, "amber")}
                {summary_card("HUMAN WAIT", summary.human_action_required, "violet")}
                {summary_card("FAILED", summary.failed, "red")}
            }

            div { class: "board-panel",
                div { class: "board-toolbar",
                    div { class: "filters", {filters} }
                    span { class: "attention-count", "要確認 {summary.needs_attention}" }
                }
                if has_rows {
                    div { class: "table-scroll",
                        table {
                            thead {
                                tr {
                                    th { "TASK" }
                                    th { "TASK STATE" }
                                    th { "RUN STATE" }
                                    th { "HEALTH" }
                                    th { "ACTION" }
                                }
                            }
                            tbody { {rows} }
                        }
                    }
                } else {
                    div { class: "empty-state",
                        span { class: "empty-glyph", "◇" }
                        h3 { "実行対象のTaskはありません" }
                        p { "フィルターを切り替えるか、IntentからTaskを準備してください。" }
                    }
                }
            }
        }
    }
}

fn summary_card(label: &str, value: usize, tone: &str) -> Element {
    rsx! {
        article { class: "summary-card {tone}",
            span { "{label}" }
            strong { "{value}" }
        }
    }
}

fn status_pill(status: &str, category: &str) -> Element {
    let normalized = status.to_ascii_lowercase().replace('_', "-");
    rsx! { span { class: "status-pill {category} {normalized}", "{status}" } }
}

fn placeholder(route: Route) -> Element {
    let (eyebrow, title, description) = match route {
        Route::Intent => (
            "DEFINE / SCOPE",
            "Intent",
            "目的と制約から、実装可能なTaskへ分解します。",
        ),
        Route::PlanReview => (
            "REVIEW / APPROVE",
            "Plan Review",
            "Task DAGと契約を確認し、実行前に承認します。",
        ),
        Route::Inbox => (
            "DECIDE / RESPOND",
            "Inbox",
            "入力要求と意思決定を、停止理由とともに扱います。",
        ),
        Route::BuildAcceptance => (
            "VERIFY / ACCEPT",
            "Build & Acceptance",
            "Build、受け入れ証跡、統合条件を確認します。",
        ),
        Route::Development => unreachable!("Development has its own view"),
    };
    rsx! {
        section { class: "page placeholder-page",
            p { class: "eyebrow", "{eyebrow}" }
            h2 { "{title}" }
            p { "{description}" }
            div { class: "placeholder-card",
                span { "NEXT VERTICAL SLICE" }
                strong { "この画面は以降のMilestoneで接続されます" }
            }
        }
    }
}

fn task_matches_filter(row: &TaskRowView, filter: TaskFilter) -> bool {
    match filter {
        TaskFilter::All => true,
        TaskFilter::Runnable => row.task_status == "READY" && row.current_run_id.is_none(),
        TaskFilter::Running => matches!(
            row.run_status.as_deref(),
            Some("PREPARING" | "AGENT_RUNNING" | "LOCAL_CHECKING")
        ),
        TaskFilter::Failed => row.run_status.as_deref() == Some("FAILED"),
        TaskFilter::HumanWait => matches!(
            row.run_status.as_deref(),
            Some("INPUT_REQUIRED" | "DECISION_REQUIRED")
        ),
    }
}

fn command_description(command: &ApplicationCommand) -> String {
    match command {
        ApplicationCommand::QueueTaskRun {
            task_id,
            expected_projection_revision,
        } => format!(
            "QueueTaskRunを準備しました: {task_id} / expected rev {expected_projection_revision}"
        ),
        ApplicationCommand::CancelTaskRun {
            task_run_id,
            expected_projection_revision,
        } => format!(
            "CancelTaskRunを準備しました: {task_run_id} / expected rev {expected_projection_revision}"
        ),
    }
}

struct ConnectionPresentation<'a> {
    label: &'static str,
    class_name: &'static str,
    disconnect_reason: Option<&'a str>,
}

fn connection_presentation(connection: &ConnectionState) -> ConnectionPresentation<'_> {
    match connection {
        ConnectionState::Connected => ConnectionPresentation {
            label: "接続中",
            class_name: "connected",
            disconnect_reason: None,
        },
        ConnectionState::Disconnected { reason } => ConnectionPresentation {
            label: "切断",
            class_name: "disconnected",
            disconnect_reason: Some(reason),
        },
    }
}
