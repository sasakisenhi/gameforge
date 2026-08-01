//! Dioxus desktop client and pure UI state transitions.

use dioxus::prelude::*;
use gameforge_application::{
    AppShellView, ApplicationCommand, BoardIntent, ConnectionState, TaskRowView,
    command_for_board_intent,
};

const APP_CSS: &str = include_str!("app.css");
const BRAND_ICON_SVG: &str = include_str!("../assets/brand-icon.svg");

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
    Queued,
    DependencyBlocked,
    Failed,
    HumanWait,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ColorTheme {
    #[default]
    Light,
    Night,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UiState {
    pub route: Route,
    pub task_filter: TaskFilter,
    pub selected_task_id: Option<String>,
    pub color_theme: ColorTheme,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiAction {
    Navigate(Route),
    SetTaskFilter(TaskFilter),
    SelectTask(Option<String>),
    ToggleTheme,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum CommandResult {
    #[default]
    Unavailable,
    Applied(Box<AppShellView>),
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandEffect {
    pub updated_view: Option<AppShellView>,
    pub notice: String,
}

#[must_use]
pub fn reduce_ui_state(mut state: UiState, action: &UiAction) -> UiState {
    match action {
        UiAction::Navigate(route) => state.route = *route,
        UiAction::SetTaskFilter(filter) => state.task_filter = *filter,
        UiAction::SelectTask(task_id) => state.selected_task_id.clone_from(task_id),
        UiAction::ToggleTheme => {
            state.color_theme = match state.color_theme {
                ColorTheme::Light => ColorTheme::Night,
                ColorTheme::Night => ColorTheme::Light,
            };
        }
    }
    state
}

#[must_use]
pub fn execute_board_intent(
    view: &AppShellView,
    intent: BoardIntent,
    execute: impl FnOnce(ApplicationCommand) -> CommandResult,
) -> CommandEffect {
    let command = match command_for_board_intent(view, intent) {
        Ok(command) => command,
        Err(error) => {
            return CommandEffect {
                updated_view: None,
                notice: format!("操作できません: {error}"),
            };
        }
    };
    let prepared = command_description(&command);
    match execute(command) {
        CommandResult::Unavailable => CommandEffect {
            updated_view: None,
            notice: prepared,
        },
        CommandResult::Applied(updated_view) => {
            let updated_view = *updated_view;
            let run_id = updated_view
                .development
                .task_rows
                .iter()
                .find_map(|row| {
                    (row.run_status.as_deref() == Some("QUEUED"))
                        .then(|| row.current_run_id.clone())
                        .flatten()
                })
                .unwrap_or_else(|| "Run ID未確認".to_owned());
            CommandEffect {
                notice: format!(
                    "Queue登録完了: {run_id} / projection rev {}",
                    updated_view.projection_revision
                ),
                updated_view: Some(updated_view),
            }
        }
        CommandResult::Failed(error) => CommandEffect {
            updated_view: None,
            notice: format!("Queue登録失敗: {error}"),
        },
    }
}

#[must_use]
pub fn render_app(view: &AppShellView) -> String {
    dioxus_ssr::render_element(rsx! { App { initial_view: view.clone() } })
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

fn brand_symbol() -> Element {
    rsx! {
        div {
            class: "brand-mark brand-symbol",
            dangerous_inner_html: "{BRAND_ICON_SVG}",
        }
    }
}

#[allow(clippy::too_many_lines)]
fn development_board(
    view: &AppShellView,
    state: &UiState,
    ui_state: Signal<UiState>,
    command_notice: Signal<Option<String>>,
    app_view: Signal<AppShellView>,
    on_command: Option<Callback<ApplicationCommand, CommandResult>>,
) -> Element {
    let summary = view.development.summary;
    let summary_cards = [
        (TaskFilter::Running, "RUNNING", summary.running, "blue", "↻"),
        (
            TaskFilter::Runnable,
            "RUNNABLE",
            summary.runnable,
            "green",
            "→",
        ),
        (TaskFilter::Queued, "QUEUED", summary.queued, "slate", "≡"),
        (
            TaskFilter::DependencyBlocked,
            "DEPENDENCY",
            summary.dependency_blocked,
            "amber",
            "⌁",
        ),
        (
            TaskFilter::HumanWait,
            "HUMAN WAIT",
            summary.human_action_required,
            "violet",
            "!",
        ),
        (TaskFilter::Failed, "FAILED", summary.failed, "red", "△"),
    ]
    .into_iter()
    .map(|(filter, label, value, tone, icon)| {
        let mut state_signal = ui_state;
        let active = state.task_filter == filter;
        let emphasized = filter == TaskFilter::Runnable && value > 0;
        let class_name = format!(
            "summary-card {tone}{}{}",
            if active { " active" } else { "" },
            if emphasized { " primary" } else { "" },
        );
        rsx! {
            button {
                class: "{class_name}",
                aria_pressed: "{active}",
                title: "Filter by {label}",
                onclick: move |_| {
                    let next = reduce_ui_state(
                        state_signal.read().clone(),
                        &UiAction::SetTaskFilter(filter),
                    );
                    state_signal.set(next);
                },
                span { class: "summary-icon", "{icon}" }
                span { class: "summary-label", "{label}" }
                strong { "{value}" }
            }
        }
    });
    let filters = [
        (TaskFilter::All, "All"),
        (TaskFilter::Runnable, "Runnable"),
        (TaskFilter::Running, "Running"),
        (TaskFilter::Queued, "Queued"),
        (TaskFilter::DependencyBlocked, "Dependency"),
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
    let selected_row = state.selected_task_id.as_deref().and_then(|selected_id| {
        view.development
            .task_rows
            .iter()
            .find(|row| row.task_id == selected_id)
            .cloned()
    });
    let selected_health_label = selected_row
        .as_ref()
        .map(|row| row.health_flags.join(", "))
        .unwrap_or_default();
    let next_runnable = view
        .development
        .task_rows
        .iter()
        .find(|row| row.can_queue)
        .cloned();
    let rows = visible_rows.into_iter().map(|row| {
        let selected = state.selected_task_id.as_deref() == Some(row.task_id.as_str());
        let row_class = match (selected, row.can_queue) {
            (true, true) => "selected runnable",
            (true, false) => "selected",
            (false, true) => "runnable",
            (false, false) => "",
        };
        let selection_id = row.task_id.clone();
        let link_selection_id = selection_id.clone();
        let queue_id = row.task_id.clone();
        let command_view = view.clone();
        let mut state_signal = ui_state;
        let mut link_state_signal = ui_state;
        let mut notice_signal = command_notice;
        let mut view_signal = app_view;
        let command_callback = on_command;
        let unavailable_reason = row.queue_unavailable_reason.clone().unwrap_or_default();
        let health_label = row.health_flags.join(", ");
        rsx! {
            tr {
                key: "{row.task_id}",
                class: "{row_class}",
                aria_selected: "{selected}",
                onclick: move |_| {
                    let next = reduce_ui_state(
                        state_signal.read().clone(),
                        &UiAction::SelectTask(Some(selection_id.clone())),
                    );
                    state_signal.set(next);
                },
                td { class: "task-identity",
                    strong { "{row.task_id}" }
                    button {
                        class: "task-link",
                        title: "Open task details",
                        onclick: move |event| {
                            event.stop_propagation();
                            let next = reduce_ui_state(
                                link_state_signal.read().clone(),
                                &UiAction::SelectTask(Some(link_selection_id.clone())),
                            );
                            link_state_signal.set(next);
                        },
                        "{row.title}"
                        span { "→" }
                    }
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
                        onclick: move |event| {
                            event.stop_propagation();
                            let effect = execute_board_intent(
                                &command_view,
                                BoardIntent::QueueTask { task_id: queue_id.clone() },
                                |command| command_callback.map_or(
                                    CommandResult::Unavailable,
                                    |callback| callback.call(command),
                                ),
                            );
                            if let Some(updated_view) = effect.updated_view {
                                view_signal.set(updated_view);
                            }
                            notice_signal.set(Some(effect.notice));
                        },
                        "Queue Task"
                    }
                }
            }
        }
    });
    let mut next_notice_signal = command_notice;
    let mut next_view_signal = app_view;
    let next_command_view = view.clone();
    let next_command_callback = on_command;
    let mut inspector_state_signal = ui_state;

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
                div {
                    class: "revision-stamp",
                    title: "Current read model projection revision",
                    "PROJECTION · REV {view.development.projection_revision}"
                }
            }

            if let Some(next_task) = next_runnable {
                section { class: "next-action", aria_label: "Next action",
                    div { class: "next-action-marker", "→" }
                    div { class: "next-action-copy",
                        span { "NEXT ACTION" }
                        strong { "{next_task.task_id} · {next_task.title}" }
                        p { "This task is ready to enter the execution queue." }
                    }
                    button {
                        class: "primary-action",
                        onclick: move |_| {
                            let effect = execute_board_intent(
                                &next_command_view,
                                BoardIntent::QueueTask {
                                    task_id: next_task.task_id.clone(),
                                },
                                |command| next_command_callback.map_or(
                                    CommandResult::Unavailable,
                                    |callback| callback.call(command),
                                ),
                            );
                            if let Some(updated_view) = effect.updated_view {
                                next_view_signal.set(updated_view);
                            }
                            next_notice_signal.set(Some(effect.notice));
                        },
                        "Queue Task"
                        span { "→" }
                    }
                }
            }

            div { class: "summary-grid",
                {summary_cards}
            }

            div { class: "board-panel",
                div { class: "board-toolbar",
                    div { class: "filters", {filters} }
                    span { class: "attention-count", "NEEDS REVIEW · {summary.needs_attention}" }
                }
                if has_rows {
                    div { class: "table-scroll",
                        table {
                            thead {
                                tr {
                                    th { "TASK" }
                                    th { "TASK STATUS" }
                                    th { "RUN STATUS" }
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

            if let Some(selected) = selected_row {
                section { class: "task-inspector", aria_label: "Selected task details",
                    header {
                        div {
                            span { class: "inspector-label", "TASK INSPECTOR" }
                            h3 { "{selected.title}" }
                            p { "{selected.task_id}" }
                        }
                        button {
                            class: "inspector-close",
                            title: "Close task details",
                            onclick: move |_| {
                                let next = reduce_ui_state(
                                    inspector_state_signal.read().clone(),
                                    &UiAction::SelectTask(None),
                                );
                                inspector_state_signal.set(next);
                            },
                            "Close ×"
                        }
                    }
                    dl {
                        div {
                            dt { "TASK STATUS" }
                            dd { {status_pill(&selected.task_status, "task")} }
                        }
                        div {
                            dt { "CURRENT RUN" }
                            dd {
                                if let Some(run_id) = selected.current_run_id.as_deref() {
                                    "{run_id}"
                                } else {
                                    span { class: "muted", "Not started" }
                                }
                            }
                        }
                        div {
                            dt { "HEALTH" }
                            dd {
                                if selected.health_flags.is_empty() {
                                    span { class: "health-ok", "HEALTHY" }
                                } else {
                                    span { class: "health-warning", "{selected_health_label}" }
                                }
                            }
                        }
                    }
                    p { class: "inspector-note",
                        "Contract、受け入れ基準、依存関係はTask detail projectionの接続後に表示されます。"
                    }
                }
            } else if has_rows {
                div { class: "board-hint",
                    span { "↗" }
                    p {
                        strong { "Select a task to inspect it" }
                        "Task名または行を選択すると、状態と実行情報を確認できます。"
                    }
                }
            }
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
        TaskFilter::Queued => row.run_status.as_deref() == Some("QUEUED"),
        TaskFilter::DependencyBlocked => row.task_status == "WAITING_DEPENDENCY",
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
