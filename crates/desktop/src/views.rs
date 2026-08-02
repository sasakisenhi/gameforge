//! Development board, inbox, and shared presentation components.

use crate::{
    CommandResult, Route, TaskFilter, UiAction, UiState, execute_board_intent,
    execute_inbox_intent, reduce_ui_state,
};
use dioxus::prelude::*;
use gameforge_application::{
    AppShellView, ApplicationCommand, BoardIntent, ConnectionState, InboxIntent, InboxItemView,
    TaskRowView,
};

const BRAND_ICON_SVG: &str = include_str!("../assets/brand-icon.svg");

pub(crate) fn brand_symbol() -> Element {
    rsx! {
        div {
            class: "brand-mark brand-symbol",
            dangerous_inner_html: "{BRAND_ICON_SVG}",
        }
    }
}

#[allow(clippy::too_many_lines)]
pub(crate) fn development_board(
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
        let cancel_id = row.current_run_id.clone();
        let command_view = view.clone();
        let mut state_signal = ui_state;
        let mut link_state_signal = ui_state;
        let mut notice_signal = command_notice;
        let mut view_signal = app_view;
        let command_callback = on_command;
        let unavailable_reason = row.queue_unavailable_reason.clone().unwrap_or_default();
        let cancel_unavailable_reason = row.cancel_unavailable_reason.clone().unwrap_or_default();
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
                    if let Some(run_id) = cancel_id {
                        if row.run_status.as_deref() == Some("FAILED") {
                            button {
                                class: "retry-button",
                                disabled: !row.can_queue,
                                title: "FAILED Runを新しいAttemptとして再実行",
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
                                "Retry Run"
                            }
                        } else {
                        button {
                            class: "cancel-button",
                            disabled: !row.can_cancel,
                            title: "{cancel_unavailable_reason}",
                            onclick: move |event| {
                                event.stop_propagation();
                                let effect = execute_board_intent(
                                    &command_view,
                                    BoardIntent::CancelRun { task_run_id: run_id.clone() },
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
                            "Cancel Run"
                        }
                        }
                    } else {
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
                        div {
                            dt { "RUN STATUS" }
                            dd {
                                if let Some(run_status) = selected.run_status.as_deref() {
                                    {status_pill(run_status, "run")}
                                } else {
                                    span { class: "muted", "Not started" }
                                }
                            }
                        }
                    }
                    div { class: "artifact-links",
                        span { class: "inspector-label", "RUN ARTIFACTS" }
                        if let Some(path) = selected.artifacts.worktree_path.as_deref() {
                            a {
                                class: "artifact-link",
                                href: "file://{path}",
                                "WORKTREE"
                                span { "↗" }
                            }
                        }
                        if let Some(path) = selected.artifacts.runtime_log_path.as_deref() {
                            a {
                                class: "artifact-link",
                                href: "file://{path}",
                                "RUNTIME LOG"
                                span { "↗" }
                            }
                        }
                        if let Some(path) = selected.artifacts.diff_path.as_deref() {
                            a {
                                class: "artifact-link",
                                href: "file://{path}",
                                "DIFF"
                                span { "↗" }
                            }
                        }
                    }
                    p { class: "inspector-note",
                        if selected.run_status.as_deref() == Some("FAILED") {
                            "このRunは失敗しました。HEALTH欄の失敗理由とruntime logを確認してから再実行してください。"
                        } else if selected.run_status.as_deref() == Some("LOCAL_CHECKING") {
                            "Local Checkを実行中です。runtime logから進行状況を確認できます。"
                        } else if selected.current_run_id.is_some() {
                            "Run artifactはCoordinatorが保持するworktreeとruntime記録を参照します。"
                        } else {
                            "Runが開始されると、worktree・runtime log・diffへの導線が表示されます。"
                        }
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

pub(crate) fn inbox_page(
    view: &AppShellView,
    command_notice: Option<Signal<Option<String>>>,
    app_view: Option<Signal<AppShellView>>,
    on_command: Option<Callback<ApplicationCommand, CommandResult>>,
) -> Element {
    let has_items = !view.inbox.items.is_empty();
    let mutations_available =
        matches!(view.connection, ConnectionState::Connected) && !view.is_stale;
    let items = view.inbox.items.iter().map(|item| {
        inbox_item(
            item,
            view,
            mutations_available,
            command_notice,
            app_view,
            on_command,
        )
    });
    rsx! {
        section { class: "page inbox-page",
            div { class: "page-heading",
                div {
                    p { class: "eyebrow", "DECIDE / RESPOND" }
                    h2 { "Inbox" }
                    p { class: "page-description",
                        "Agentが停止して待っている入力要求を、Task Runと結び付けて確認します。"
                    }
                }
                div { class: "revision-stamp", "ACTION INBOX · {view.inbox.pending} PENDING" }
            }
            if has_items {
                div { class: "inbox-list", {items} }
            } else {
                div { class: "empty-state inbox-empty",
                    span { class: "empty-glyph", "✓" }
                    h3 { "対応待ちの入力要求はありません" }
                    p { "Agentから入力要求が届くと、ここにTaskとRunを表示します。" }
                }
            }
        }
    }
}

fn inbox_item(
    item: &InboxItemView,
    view: &AppShellView,
    mutations_available: bool,
    command_notice: Option<Signal<Option<String>>>,
    app_view: Option<Signal<AppShellView>>,
    on_command: Option<Callback<ApplicationCommand, CommandResult>>,
) -> Element {
    let request_id = item.request_id.clone();
    let command_view = view.clone();
    rsx! {
        article { class: "inbox-item", key: "{item.request_id}",
            header {
                div {
                    span { class: "inbox-kind", "{item.request_kind}" }
                    strong { "{item.request_id}" }
                }
                {status_pill(&item.status, "request")}
            }
            p { class: "inbox-prompt", "{item.prompt}" }
            dl {
                div {
                    dt { "TASK" }
                    dd { "{item.task_id}" }
                }
                div {
                    dt { "RUN" }
                    dd { "{item.task_run_id}" }
                }
                div {
                    dt { "REQUESTED" }
                    dd { "{item.requested_at}" }
                }
            }
            if item.status == "PENDING" {
                form {
                    class: "answer-form",
                    onsubmit: move |event| {
                        event.prevent_default();
                        let answer = match event.get_first("answer") {
                            Some(FormValue::Text(answer)) => answer,
                            _ => String::new(),
                        };
                        let effect = execute_inbox_intent(
                            &command_view,
                            InboxIntent::AnswerInput {
                                request_id: request_id.clone(),
                                answer,
                            },
                            |command| on_command.map_or(
                                CommandResult::Unavailable,
                                |callback| callback.call(command),
                            ),
                        );
                        if let (Some(updated_view), Some(mut signal)) =
                            (effect.updated_view, app_view)
                        {
                            signal.set(updated_view);
                        }
                        if let Some(mut signal) = command_notice {
                            signal.set(Some(effect.notice));
                        }
                    },
                    textarea {
                        name: "answer",
                        required: true,
                        disabled: !mutations_available,
                        placeholder: "Agentへ返す回答を入力してください",
                    }
                    button {
                        r#type: "submit",
                        disabled: !mutations_available,
                        "Submit Answer"
                    }
                }
            } else {
                p { class: "answer-complete", "この要求には回答済みです。" }
            }
        }
    }
}

pub(crate) fn placeholder(route: Route) -> Element {
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
        Route::Inbox => unreachable!("Inbox has its own view"),
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

pub(crate) struct ConnectionPresentation<'a> {
    pub(crate) label: &'static str,
    pub(crate) class_name: &'static str,
    pub(crate) disconnect_reason: Option<&'a str>,
}

pub(crate) fn connection_presentation(connection: &ConnectionState) -> ConnectionPresentation<'_> {
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
