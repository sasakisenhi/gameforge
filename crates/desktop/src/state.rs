use gameforge_application::{
    AppShellView, ApplicationCommand, BoardIntent, InboxIntent, command_for_board_intent,
    command_for_inbox_intent,
};

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
    let target = match &command {
        ApplicationCommand::AddTaskFromConversation { task_id, .. } => {
            CommandTarget::QueueTask(task_id.clone())
        }
        ApplicationCommand::QueueTaskRun { task_id, .. } => {
            CommandTarget::QueueTask(task_id.clone())
        }
        ApplicationCommand::CancelTaskRun { task_run_id, .. } => {
            CommandTarget::CancelRun(task_run_id.clone())
        }
        ApplicationCommand::AnswerInputRequest { request_id, .. } => {
            CommandTarget::AnswerInput(request_id.clone())
        }
    };
    match execute(command) {
        CommandResult::Unavailable => CommandEffect {
            updated_view: None,
            notice: prepared,
        },
        CommandResult::Applied(updated_view) => {
            let updated_view = *updated_view;
            let notice = match target {
                CommandTarget::QueueTask(task_id) => {
                    let run = updated_view
                        .development
                        .task_rows
                        .iter()
                        .find(|row| row.task_id == task_id);
                    let run_id = run
                        .and_then(|row| row.current_run_id.as_deref())
                        .unwrap_or("Run ID未確認");
                    let run_status = run
                        .and_then(|row| row.run_status.as_deref())
                        .unwrap_or("状態未確認");
                    format!(
                        "Queue登録完了: {run_id} / {run_status} / projection rev {}",
                        updated_view.projection_revision
                    )
                }
                CommandTarget::CancelRun(run_id) => format!(
                    "Run取消し完了: {run_id} / CANCELLED / projection rev {}",
                    updated_view.projection_revision
                ),
                CommandTarget::AnswerInput(request_id) => format!(
                    "入力回答完了: {request_id} / projection rev {}",
                    updated_view.projection_revision
                ),
            };
            CommandEffect {
                notice,
                updated_view: Some(updated_view),
            }
        }
        CommandResult::Failed(error) => CommandEffect {
            updated_view: None,
            notice: match target {
                CommandTarget::QueueTask(_) => format!("Queue登録失敗: {error}"),
                CommandTarget::CancelRun(_) => format!("Run取消し失敗: {error}"),
                CommandTarget::AnswerInput(_) => format!("入力回答失敗: {error}"),
            },
        },
    }
}

#[must_use]
pub fn execute_inbox_intent(
    view: &AppShellView,
    intent: InboxIntent,
    execute: impl FnOnce(ApplicationCommand) -> CommandResult,
) -> CommandEffect {
    let command = match command_for_inbox_intent(view, intent) {
        Ok(command) => command,
        Err(error) => {
            return CommandEffect {
                updated_view: None,
                notice: format!("回答できません: {error}"),
            };
        }
    };
    let ApplicationCommand::AnswerInputRequest { request_id, .. } = &command else {
        return CommandEffect {
            updated_view: None,
            notice: "回答Commandを作成できませんでした".to_owned(),
        };
    };
    let request_id = request_id.clone();
    match execute(command) {
        CommandResult::Unavailable => CommandEffect {
            updated_view: None,
            notice: format!("入力回答を準備しました: {request_id}"),
        },
        CommandResult::Applied(updated_view) => {
            let updated_view = *updated_view;
            let run_status = updated_view
                .inbox
                .items
                .iter()
                .find(|item| item.request_id == request_id)
                .and_then(|item| {
                    updated_view.development.task_rows.iter().find(|row| {
                        row.current_run_id.as_deref() == Some(item.task_run_id.as_str())
                    })
                })
                .and_then(|row| row.run_status.as_deref())
                .unwrap_or("状態未確認");
            CommandEffect {
                notice: format!(
                    "入力回答完了: {request_id} / {run_status} / projection rev {}",
                    updated_view.projection_revision
                ),
                updated_view: Some(updated_view),
            }
        }
        CommandResult::Failed(error) => CommandEffect {
            updated_view: None,
            notice: format!("入力回答失敗: {error}"),
        },
    }
}

enum CommandTarget {
    QueueTask(String),
    CancelRun(String),
    AnswerInput(String),
}

fn command_description(command: &ApplicationCommand) -> String {
    match command {
        ApplicationCommand::AddTaskFromConversation { task_id, .. } => {
            format!("Task {task_id} を会話から追加")
        }
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
        ApplicationCommand::AnswerInputRequest {
            request_id,
            expected_projection_revision,
            ..
        } => format!(
            "AnswerInputRequestを準備しました: {request_id} / expected rev {expected_projection_revision}"
        ),
    }
}
