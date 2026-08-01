use std::fmt;

use gameforge_persistence::{DevelopmentBoardRow, InboxRow};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectValidation {
    pub task_count: usize,
    pub integration_order: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectSnapshot {
    pub projection_revision: u64,
    pub development_board: Vec<DevelopmentBoardRow>,
    pub inbox: Vec<InboxRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandContext {
    pub command_id: String,
    pub actor: String,
    pub occurred_at: String,
    pub base_commit: String,
}

impl CommandContext {
    #[must_use]
    pub fn child(&self, operation: &str) -> Self {
        Self {
            command_id: format!("{}/{operation}", self.command_id),
            actor: self.actor.clone(),
            occurred_at: self.occurred_at.clone(),
            base_commit: self.base_commit.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootstrapError {
    Io(String),
    NoTaskDocuments,
    Document(String),
    Journal(String),
    Projection(String),
    Coordinator(String),
    RunExecution(String),
    InvalidCommand(String),
    UnsupportedCommand(String),
    CommandIdConflict(String),
    ProjectionRevisionConflict {
        expected: u64,
        actual: u64,
    },
    TaskNotFound(String),
    TaskNotReady(String),
    TaskAlreadyHasRun(String),
    RunNotFound(String),
    RunNotPreparing {
        run_id: String,
        actual: Option<String>,
    },
    OperationOutcomeUnknown(String),
    InputRequestAlreadyExists(String),
    InputRequestNotFound(String),
    InputRequestNotPending {
        request_id: String,
        actual: String,
    },
    RunStateConflict {
        run_id: String,
        expected: &'static str,
        actual: Option<String>,
    },
}

impl fmt::Display for BootstrapError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "project I/O failed: {error}"),
            Self::NoTaskDocuments => formatter.write_str("no Task documents were found"),
            Self::Document(error) => write!(formatter, "Task document validation failed: {error}"),
            Self::Journal(error) => write!(formatter, "Event Journal failed: {error}"),
            Self::Projection(error) => write!(formatter, "Read Model failed: {error}"),
            Self::Coordinator(error) => write!(formatter, "ProjectCoordinator failed: {error}"),
            Self::RunExecution(error) => write!(formatter, "Run execution failed: {error}"),
            Self::InvalidCommand(error) => write!(formatter, "invalid command: {error}"),
            Self::UnsupportedCommand(command) => {
                write!(formatter, "unsupported command: {command}")
            }
            Self::CommandIdConflict(command_id) => write!(
                formatter,
                "command ID was reused with different content: {command_id}"
            ),
            Self::ProjectionRevisionConflict { expected, actual } => write!(
                formatter,
                "projection revision conflict: expected {expected}, actual {actual}"
            ),
            Self::TaskNotFound(task_id) => write!(formatter, "Task not found: {task_id}"),
            Self::TaskNotReady(task_id) => write!(formatter, "Task is not READY: {task_id}"),
            Self::TaskAlreadyHasRun(task_id) => {
                write!(formatter, "Task already has a Run: {task_id}")
            }
            Self::RunNotFound(run_id) => write!(formatter, "Task Run not found: {run_id}"),
            Self::RunNotPreparing { run_id, actual } => write!(
                formatter,
                "Task Run is not PREPARING: {run_id} ({})",
                actual.as_deref().unwrap_or("NO_RUN_STATUS")
            ),
            Self::OperationOutcomeUnknown(command_id) => write!(
                formatter,
                "external operation outcome is unknown: {command_id}"
            ),
            Self::InputRequestAlreadyExists(request_id) => {
                write!(formatter, "Input request already exists: {request_id}")
            }
            Self::InputRequestNotFound(request_id) => {
                write!(formatter, "Input request not found: {request_id}")
            }
            Self::InputRequestNotPending { request_id, actual } => write!(
                formatter,
                "Input request is not PENDING: {request_id} ({actual})"
            ),
            Self::RunStateConflict {
                run_id,
                expected,
                actual,
            } => write!(
                formatter,
                "Task Run state conflict: {run_id} expected {expected}, actual {}",
                actual.as_deref().unwrap_or("NO_RUN_STATUS")
            ),
        }
    }
}

impl std::error::Error for BootstrapError {}

impl From<std::io::Error> for BootstrapError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}
