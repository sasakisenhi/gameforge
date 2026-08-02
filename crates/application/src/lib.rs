//! Framework-independent application commands, queries, and View DTOs.
#![allow(clippy::missing_errors_doc)]

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
    Connected,
    Disconnected { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppShellContext {
    pub project_name: String,
    pub project_root: String,
    pub main_commit: String,
    pub connection: ConnectionState,
    pub projection_revision: u64,
    pub last_synced_at: String,
    pub is_stale: bool,
    pub inbox_count: usize,
    pub max_concurrent_task_runs: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevelopmentBoardRecord {
    pub task_id: String,
    pub title: String,
    pub task_status: String,
    pub current_run_id: Option<String>,
    pub run_status: Option<String>,
    pub health_flags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboxItemRecord {
    pub request_id: String,
    pub task_id: String,
    pub task_run_id: String,
    pub request_kind: String,
    pub prompt: String,
    pub status: String,
    pub requested_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRowView {
    pub task_id: String,
    pub title: String,
    pub task_status: String,
    pub current_run_id: Option<String>,
    pub run_status: Option<String>,
    pub health_flags: Vec<String>,
    pub can_queue: bool,
    pub queue_unavailable_reason: Option<String>,
    pub can_cancel: bool,
    pub cancel_unavailable_reason: Option<String>,
    pub artifacts: TaskArtifactsView,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskArtifactsView {
    pub worktree_path: Option<String>,
    pub runtime_log_path: Option<String>,
    pub diff_path: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DevelopmentBoardSummary {
    pub running: usize,
    pub runnable: usize,
    pub queued: usize,
    pub dependency_blocked: usize,
    pub human_action_required: usize,
    pub failed: usize,
    pub needs_attention: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevelopmentBoardView {
    pub projection_revision: u64,
    pub summary: DevelopmentBoardSummary,
    pub task_rows: Vec<TaskRowView>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboxItemView {
    pub request_id: String,
    pub task_id: String,
    pub task_run_id: String,
    pub request_kind: String,
    pub prompt: String,
    pub status: String,
    pub requested_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboxView {
    pub projection_revision: u64,
    pub pending: usize,
    pub items: Vec<InboxItemView>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskGenerationPreview {
    pub task_id: String,
    pub title: String,
    pub purpose: String,
    pub acceptance_criteria: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppShellView {
    pub project_name: String,
    pub project_root: String,
    pub main_commit: String,
    pub connection: ConnectionState,
    pub projection_revision: u64,
    pub last_synced_at: String,
    pub is_stale: bool,
    pub inbox_count: usize,
    pub max_concurrent_task_runs: usize,
    pub development: DevelopmentBoardView,
    pub inbox: InboxView,
}

#[must_use]
pub fn compose_app_shell(
    context: AppShellContext,
    records: Vec<DevelopmentBoardRecord>,
) -> AppShellView {
    compose_app_shell_with_inbox(context, records, Vec::new())
}

#[must_use]
pub fn compose_app_shell_with_inbox(
    context: AppShellContext,
    records: Vec<DevelopmentBoardRecord>,
    inbox_records: Vec<InboxItemRecord>,
) -> AppShellView {
    let mut summary = DevelopmentBoardSummary::default();
    let mutations_available =
        matches!(context.connection, ConnectionState::Connected) && !context.is_stale;
    let task_rows = records
        .into_iter()
        .map(|record| {
            let run_status = record.run_status.as_deref();
            if matches!(
                run_status,
                Some("PREPARING" | "AGENT_RUNNING" | "LOCAL_CHECKING")
            ) {
                summary.running += 1;
            }
            if run_status == Some("QUEUED") {
                summary.queued += 1;
            }
            if record.task_status == "WAITING_DEPENDENCY" {
                summary.dependency_blocked += 1;
            }
            if matches!(run_status, Some("INPUT_REQUIRED" | "DECISION_REQUIRED")) {
                summary.human_action_required += 1;
            }
            if run_status == Some("FAILED") {
                summary.failed += 1;
            }
            if run_status == Some("FAILED") || !record.health_flags.is_empty() {
                summary.needs_attention += 1;
            }

            let task_is_runnable = record.task_status == "READY" && record.current_run_id.is_none();
            let failed_run_is_retryable = run_status == Some("FAILED");
            if task_is_runnable {
                summary.runnable += 1;
            }
            let can_queue = mutations_available && (task_is_runnable || failed_run_is_retryable);
            let queue_unavailable_reason = (!can_queue).then(|| {
                if !mutations_available {
                    "Coordinatorへ接続し、最新Projectionを取得してください".to_owned()
                } else if failed_run_is_retryable {
                    "FAILED Runを新しいAttemptとして再実行できます".to_owned()
                } else if record.current_run_id.is_some() {
                    "このTaskには進行中または記録済みのRunがあります".to_owned()
                } else {
                    format!("Task状態 {} ではQueueできません", record.task_status)
                }
            });
            let run_is_cancellable = record.current_run_id.is_some()
                && matches!(
                    run_status,
                    Some(
                        "QUEUED"
                            | "PREPARING"
                            | "AGENT_RUNNING"
                            | "INPUT_REQUIRED"
                            | "DECISION_REQUIRED"
                            | "LOCAL_CHECKING"
                    )
                );
            let can_cancel = mutations_available && run_is_cancellable;
            let cancel_unavailable_reason =
                (!can_cancel && record.current_run_id.is_some()).then(|| {
                    if mutations_available {
                        format!(
                            "Run状態 {} では取消しできません",
                            run_status.unwrap_or("未確認")
                        )
                    } else {
                        "Coordinatorへ接続し、最新Projectionを取得してください".to_owned()
                    }
                });

            let artifacts = record.current_run_id.as_deref().map_or_else(
                || TaskArtifactsView {
                    worktree_path: None,
                    runtime_log_path: None,
                    diff_path: None,
                },
                |run_id| TaskArtifactsView {
                    worktree_path: Some(format!(
                        "{}/.game-dev/worktrees/{run_id}",
                        context.project_root
                    )),
                    runtime_log_path: Some(format!(
                        "{}/.game-dev/runtime/runs/{run_id}",
                        context.project_root
                    )),
                    diff_path: Some(format!(
                        "{}/.game-dev/runtime/runs/{run_id}/diff.patch",
                        context.project_root
                    )),
                },
            );

            TaskRowView {
                task_id: record.task_id,
                title: record.title,
                task_status: record.task_status,
                current_run_id: record.current_run_id,
                run_status: record.run_status,
                health_flags: record.health_flags,
                can_queue,
                queue_unavailable_reason,
                can_cancel,
                cancel_unavailable_reason,
                artifacts,
            }
        })
        .collect();
    let inbox = compose_inbox(context.projection_revision, inbox_records);

    AppShellView {
        project_name: context.project_name,
        project_root: context.project_root,
        main_commit: context.main_commit,
        connection: context.connection,
        projection_revision: context.projection_revision,
        last_synced_at: context.last_synced_at,
        is_stale: context.is_stale,
        inbox_count: inbox.pending,
        max_concurrent_task_runs: context.max_concurrent_task_runs,
        development: DevelopmentBoardView {
            projection_revision: context.projection_revision,
            summary,
            task_rows,
        },
        inbox,
    }
}

fn compose_inbox(projection_revision: u64, records: Vec<InboxItemRecord>) -> InboxView {
    let items = records
        .into_iter()
        .map(|record| InboxItemView {
            request_id: record.request_id,
            task_id: record.task_id,
            task_run_id: record.task_run_id,
            request_kind: record.request_kind,
            prompt: record.prompt,
            status: record.status,
            requested_at: record.requested_at,
        })
        .collect::<Vec<_>>();
    let pending = items.iter().filter(|item| item.status == "PENDING").count();
    InboxView {
        projection_revision,
        pending,
        items,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoardIntent {
    QueueTask { task_id: String },
    CancelRun { task_run_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InboxIntent {
    AnswerInput { request_id: String, answer: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplicationCommand {
    AddTaskFromConversation {
        task_id: String,
        request: String,
        expected_projection_revision: u64,
    },
    PromoteTaskToReady {
        task_id: String,
        expected_projection_revision: u64,
    },
    QueueTaskRun {
        task_id: String,
        expected_projection_revision: u64,
    },
    CancelTaskRun {
        task_run_id: String,
        expected_projection_revision: u64,
    },
    AnswerInputRequest {
        request_id: String,
        answer: String,
        expected_projection_revision: u64,
    },
}

pub fn preview_task_generation(
    request: &str,
    next_task_id: &str,
) -> Result<TaskGenerationPreview, ActionError> {
    if request.trim().is_empty() {
        return Err(ActionError::ActionUnavailable {
            reason: "実装したい内容を入力してください".to_owned(),
        });
    }
    let title = request
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .chars()
        .take(80)
        .collect::<String>();
    Ok(TaskGenerationPreview {
        task_id: next_task_id.to_owned(),
        title: if title.is_empty() {
            "Generated task".to_owned()
        } else {
            title
        },
        purpose: request.trim().to_owned(),
        acceptance_criteria: vec![format!("{next_task_id}-ACCEPTANCE")],
    })
}

pub fn command_for_task_generation(
    view: &AppShellView,
    request: &str,
    task_id: &str,
) -> Result<ApplicationCommand, ActionError> {
    if !matches!(view.connection, ConnectionState::Connected) {
        return Err(ActionError::CoordinatorDisconnected);
    }
    if view.is_stale {
        return Err(ActionError::StaleProjection);
    }
    preview_task_generation(request, task_id)?;
    if view
        .development
        .task_rows
        .iter()
        .any(|row| row.task_id == task_id)
    {
        return Err(ActionError::ActionUnavailable {
            reason: format!("Task IDが既に存在します: {task_id}"),
        });
    }
    Ok(ApplicationCommand::AddTaskFromConversation {
        task_id: task_id.to_owned(),
        request: request.trim().to_owned(),
        expected_projection_revision: view.projection_revision,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionError {
    CoordinatorDisconnected,
    StaleProjection,
    TaskNotFound(String),
    RunNotFound(String),
    InputRequestNotFound(String),
    ActionUnavailable { reason: String },
}

impl fmt::Display for ActionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CoordinatorDisconnected => formatter.write_str("Coordinator is disconnected"),
            Self::StaleProjection => formatter.write_str("Projection is stale"),
            Self::TaskNotFound(task_id) => write!(formatter, "Task not found: {task_id}"),
            Self::RunNotFound(run_id) => write!(formatter, "Task Run not found: {run_id}"),
            Self::InputRequestNotFound(request_id) => {
                write!(formatter, "Input request not found: {request_id}")
            }
            Self::ActionUnavailable { reason } => {
                write!(formatter, "action is unavailable: {reason}")
            }
        }
    }
}

impl std::error::Error for ActionError {}

pub fn command_for_inbox_intent(
    view: &AppShellView,
    intent: InboxIntent,
) -> Result<ApplicationCommand, ActionError> {
    if !matches!(view.connection, ConnectionState::Connected) {
        return Err(ActionError::CoordinatorDisconnected);
    }
    if view.is_stale {
        return Err(ActionError::StaleProjection);
    }

    match intent {
        InboxIntent::AnswerInput { request_id, answer } => {
            let item = view
                .inbox
                .items
                .iter()
                .find(|item| item.request_id == request_id)
                .ok_or_else(|| ActionError::InputRequestNotFound(request_id.clone()))?;
            if item.status != "PENDING" {
                return Err(ActionError::ActionUnavailable {
                    reason: format!(
                        "入力要求 {} は回答待ちではありません: {}",
                        item.request_id, item.status
                    ),
                });
            }
            if answer.trim().is_empty() {
                return Err(ActionError::ActionUnavailable {
                    reason: "回答を入力してください".to_owned(),
                });
            }
            Ok(ApplicationCommand::AnswerInputRequest {
                request_id,
                answer,
                expected_projection_revision: view.projection_revision,
            })
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunLaunchRequest {
    pub task_run_id: String,
    pub task_id: String,
    pub contract_revision: u64,
    pub base_commit: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartedRun {
    resource_lease: String,
    worktree_lease: String,
    agent_session: String,
}

impl StartedRun {
    pub fn new(
        resource_lease_id: impl Into<String>,
        worktree_lease_id: impl Into<String>,
        agent_session_id: impl Into<String>,
    ) -> Result<Self, RunExecutionContractError> {
        Ok(Self {
            resource_lease: required_execution_value(
                "resource_lease_id",
                resource_lease_id.into(),
            )?,
            worktree_lease: required_execution_value(
                "worktree_lease_id",
                worktree_lease_id.into(),
            )?,
            agent_session: required_execution_value("agent_session_id", agent_session_id.into())?,
        })
    }

    #[must_use]
    pub fn resource_lease_id(&self) -> &str {
        &self.resource_lease
    }

    #[must_use]
    pub fn worktree_lease_id(&self) -> &str {
        &self.worktree_lease
    }

    #[must_use]
    pub fn agent_session_id(&self) -> &str {
        &self.agent_session
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunLaunchDeferral {
    ResourceUnavailable,
    WorktreeUnavailable,
    AgentUnavailable,
}

impl RunLaunchDeferral {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ResourceUnavailable => "RESOURCE_UNAVAILABLE",
            Self::WorktreeUnavailable => "WORKTREE_UNAVAILABLE",
            Self::AgentUnavailable => "AGENT_UNAVAILABLE",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunLaunchOutcome {
    Started(StartedRun),
    Deferred {
        reason: RunLaunchDeferral,
        detail: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunExecutionUpdate {
    Completed {
        task_run_id: String,
        agent_session_id: String,
        red_evidence_present: bool,
        green_evidence_present: bool,
    },
    Failed {
        task_run_id: String,
        detail: String,
    },
    InputRequired {
        task_run_id: String,
        request_id: String,
        prompt: String,
    },
}

pub trait RunExecutionPort {
    fn start_run(&mut self, request: &RunLaunchRequest) -> RunLaunchOutcome;

    fn poll_updates(&mut self) -> Vec<RunExecutionUpdate> {
        Vec::new()
    }

    fn cancel_run(&mut self, _task_run_id: &str) -> Result<(), String> {
        Ok(())
    }

    fn answer_input(
        &mut self,
        _task_run_id: &str,
        _request_id: &str,
        _answer: &str,
    ) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalVerificationRequest {
    pub task_run_id: String,
    pub task_id: String,
    pub base_commit: String,
    pub worktree_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalVerificationUpdate {
    Passed {
        task_run_id: String,
        head_commit: String,
        changed_paths: Vec<String>,
        completed_checks: Vec<String>,
        final_suite_passed: bool,
    },
    Failed {
        task_run_id: String,
        detail: String,
    },
}

/// Runs repository-local quality gates after an Agent turn completes.
///
/// Implementations start checks without blocking the coordinator and return
/// terminal results from [`Self::poll_updates`].
pub trait LocalVerificationPort {
    fn start_verification(&mut self, request: &LocalVerificationRequest) -> Result<(), String>;

    fn is_verification_active(&self, task_run_id: &str) -> bool;

    fn poll_updates(&mut self) -> Vec<LocalVerificationUpdate> {
        Vec::new()
    }

    fn cancel_verification(&mut self, _task_run_id: &str) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunExecutionContractError {
    field: &'static str,
}

impl fmt::Display for RunExecutionContractError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} must not be empty", self.field)
    }
}

impl std::error::Error for RunExecutionContractError {}

fn required_execution_value(
    field: &'static str,
    value: String,
) -> Result<String, RunExecutionContractError> {
    if value.trim().is_empty() {
        Err(RunExecutionContractError { field })
    } else {
        Ok(value)
    }
}

pub fn command_for_board_intent(
    view: &AppShellView,
    intent: BoardIntent,
) -> Result<ApplicationCommand, ActionError> {
    if !matches!(view.connection, ConnectionState::Connected) {
        return Err(ActionError::CoordinatorDisconnected);
    }
    if view.is_stale {
        return Err(ActionError::StaleProjection);
    }

    match intent {
        BoardIntent::QueueTask { task_id } => {
            let row = view
                .development
                .task_rows
                .iter()
                .find(|row| row.task_id == task_id)
                .ok_or_else(|| ActionError::TaskNotFound(task_id.clone()))?;
            if !row.can_queue {
                return Err(ActionError::ActionUnavailable {
                    reason: row
                        .queue_unavailable_reason
                        .clone()
                        .unwrap_or_else(|| "Task cannot be queued".to_owned()),
                });
            }
            Ok(ApplicationCommand::QueueTaskRun {
                task_id,
                expected_projection_revision: view.projection_revision,
            })
        }
        BoardIntent::CancelRun { task_run_id } => {
            let exists = view.development.task_rows.iter().any(|row| {
                row.current_run_id.as_deref() == Some(task_run_id.as_str())
                    && matches!(
                        row.run_status.as_deref(),
                        Some(
                            "QUEUED"
                                | "PREPARING"
                                | "AGENT_RUNNING"
                                | "INPUT_REQUIRED"
                                | "DECISION_REQUIRED"
                                | "LOCAL_CHECKING"
                        )
                    )
            });
            if !exists {
                return Err(ActionError::RunNotFound(task_run_id));
            }
            Ok(ApplicationCommand::CancelTaskRun {
                task_run_id,
                expected_projection_revision: view.projection_revision,
            })
        }
    }
}
