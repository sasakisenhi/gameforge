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
pub struct TaskRowView {
    pub task_id: String,
    pub title: String,
    pub task_status: String,
    pub current_run_id: Option<String>,
    pub run_status: Option<String>,
    pub health_flags: Vec<String>,
    pub can_queue: bool,
    pub queue_unavailable_reason: Option<String>,
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
}

#[must_use]
pub fn compose_app_shell(
    context: AppShellContext,
    records: Vec<DevelopmentBoardRecord>,
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
            if task_is_runnable {
                summary.runnable += 1;
            }
            let can_queue = mutations_available && task_is_runnable;
            let queue_unavailable_reason = (!can_queue).then(|| {
                if !mutations_available {
                    "Coordinatorへ接続し、最新Projectionを取得してください".to_owned()
                } else if record.current_run_id.is_some() {
                    "このTaskには進行中または記録済みのRunがあります".to_owned()
                } else {
                    format!("Task状態 {} ではQueueできません", record.task_status)
                }
            });

            TaskRowView {
                task_id: record.task_id,
                title: record.title,
                task_status: record.task_status,
                current_run_id: record.current_run_id,
                run_status: record.run_status,
                health_flags: record.health_flags,
                can_queue,
                queue_unavailable_reason,
            }
        })
        .collect();

    AppShellView {
        project_name: context.project_name,
        project_root: context.project_root,
        main_commit: context.main_commit,
        connection: context.connection,
        projection_revision: context.projection_revision,
        last_synced_at: context.last_synced_at,
        is_stale: context.is_stale,
        inbox_count: context.inbox_count,
        max_concurrent_task_runs: context.max_concurrent_task_runs,
        development: DevelopmentBoardView {
            projection_revision: context.projection_revision,
            summary,
            task_rows,
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BoardIntent {
    QueueTask { task_id: String },
    CancelRun { task_run_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplicationCommand {
    QueueTaskRun {
        task_id: String,
        expected_projection_revision: u64,
    },
    CancelTaskRun {
        task_run_id: String,
        expected_projection_revision: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionError {
    CoordinatorDisconnected,
    StaleProjection,
    TaskNotFound(String),
    RunNotFound(String),
    ActionUnavailable { reason: String },
}

impl fmt::Display for ActionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CoordinatorDisconnected => formatter.write_str("Coordinator is disconnected"),
            Self::StaleProjection => formatter.write_str("Projection is stale"),
            Self::TaskNotFound(task_id) => write!(formatter, "Task not found: {task_id}"),
            Self::RunNotFound(run_id) => write!(formatter, "Task Run not found: {run_id}"),
            Self::ActionUnavailable { reason } => {
                write!(formatter, "action is unavailable: {reason}")
            }
        }
    }
}

impl std::error::Error for ActionError {}

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
}

impl RunLaunchDeferral {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ResourceUnavailable => "RESOURCE_UNAVAILABLE",
            Self::WorktreeUnavailable => "WORKTREE_UNAVAILABLE",
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

pub trait RunExecutionPort {
    fn start_run(&mut self, request: &RunLaunchRequest) -> RunLaunchOutcome;
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
