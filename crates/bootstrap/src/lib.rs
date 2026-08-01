//! Shared composition root for desktop and headless coordinators.
#![allow(clippy::missing_errors_doc)]

use std::{collections::BTreeMap, fmt, fs, path::Path};

use gameforge_application::ApplicationCommand;
use gameforge_domain::{CommitSha, TaskRun, TaskRunId};
use gameforge_event_journal::{
    AggregateRef, EventEnvelope, EventHeader, EventJournal, SUPPORTED_SCHEMA_VERSION,
};
use gameforge_persistence::{DevelopmentBoardRow, ProjectionStore};
use gameforge_project_documents::{
    TaskDocument, TaskDocumentStatus, load_task_document, validate_task_documents,
};
use gameforge_runtime::ProjectWriterLease;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectValidation {
    pub task_count: usize,
    pub integration_order: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectSnapshot {
    pub projection_revision: u64,
    pub development_board: Vec<DevelopmentBoardRow>,
}

pub struct ProjectSession {
    snapshot: ProjectSnapshot,
    documents: Vec<TaskDocument>,
    journal: EventJournal,
    projection: ProjectionStore,
    _writer_lease: ProjectWriterLease,
}

impl ProjectSession {
    #[must_use]
    pub const fn snapshot(&self) -> &ProjectSnapshot {
        &self.snapshot
    }

    pub fn execute(
        &mut self,
        context: CommandContext,
        command: ApplicationCommand,
    ) -> Result<ProjectSnapshot, BootstrapError> {
        validate_command_context(&context)?;
        match command {
            ApplicationCommand::QueueTaskRun {
                task_id,
                expected_projection_revision,
            } => self.queue_task_run(context, &task_id, expected_projection_revision),
            ApplicationCommand::CancelTaskRun { .. } => Err(BootstrapError::UnsupportedCommand(
                "CancelTaskRun".to_owned(),
            )),
        }
    }

    fn queue_task_run(
        &mut self,
        context: CommandContext,
        task_id: &str,
        expected_projection_revision: u64,
    ) -> Result<ProjectSnapshot, BootstrapError> {
        if let Some(previous) = self
            .journal
            .events()
            .iter()
            .find(|event| event.header().correlation_id == context.command_id)
        {
            let same_command = previous.event_type() == "TaskRunQueued"
                && previous.payload().get("task_id").map(String::as_str) == Some(task_id);
            return if same_command {
                Ok(self.snapshot.clone())
            } else {
                Err(BootstrapError::CommandIdConflict(context.command_id))
            };
        }

        let actual_revision = self.snapshot.projection_revision;
        if expected_projection_revision != actual_revision {
            return Err(BootstrapError::ProjectionRevisionConflict {
                expected: expected_projection_revision,
                actual: actual_revision,
            });
        }

        let row = self
            .snapshot
            .development_board
            .iter()
            .find(|row| row.task_id == task_id)
            .ok_or_else(|| BootstrapError::TaskNotFound(task_id.to_owned()))?;
        if row.current_run_id.is_some() {
            return Err(BootstrapError::TaskAlreadyHasRun(task_id.to_owned()));
        }
        let document = self
            .documents
            .iter()
            .find(|document| document.id().as_str() == task_id)
            .ok_or_else(|| BootstrapError::TaskNotFound(task_id.to_owned()))?;
        if document.status() != TaskDocumentStatus::Ready {
            return Err(BootstrapError::TaskNotReady(task_id.to_owned()));
        }

        let attempt = self
            .journal
            .events()
            .iter()
            .filter(|event| {
                event.event_type() == "TaskRunQueued"
                    && event.payload().get("task_id").map(String::as_str) == Some(task_id)
            })
            .count()
            + 1;
        let run_id = format!("RUN-{task_id}-{attempt}");
        let base_commit = CommitSha::new(&context.base_commit)
            .map_err(|error| BootstrapError::InvalidCommand(error.to_string()))?;
        let task_run = TaskRun::new(
            TaskRunId::new(&run_id)
                .map_err(|error| BootstrapError::InvalidCommand(error.to_string()))?,
            document.id().clone(),
            document.contract_revision(),
            base_commit,
        );
        let event = queued_event(&context, &task_run)?;

        self.journal
            .append(&event)
            .map_err(|error| BootstrapError::Journal(error.to_string()))?;
        self.projection
            .apply_event(&event)
            .map_err(|error| BootstrapError::Projection(error.to_string()))?;
        self.refresh_snapshot()?;
        Ok(self.snapshot.clone())
    }

    fn refresh_snapshot(&mut self) -> Result<(), BootstrapError> {
        self.snapshot = ProjectSnapshot {
            projection_revision: self
                .projection
                .projection_revision()
                .map_err(|error| BootstrapError::Projection(error.to_string()))?,
            development_board: self
                .projection
                .development_board_rows()
                .map_err(|error| BootstrapError::Projection(error.to_string()))?,
        };
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandContext {
    pub command_id: String,
    pub actor: String,
    pub occurred_at: String,
    pub base_commit: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootstrapError {
    Io(String),
    NoTaskDocuments,
    Document(String),
    Journal(String),
    Projection(String),
    Coordinator(String),
    InvalidCommand(String),
    UnsupportedCommand(String),
    CommandIdConflict(String),
    ProjectionRevisionConflict { expected: u64, actual: u64 },
    TaskNotFound(String),
    TaskNotReady(String),
    TaskAlreadyHasRun(String),
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
            Self::InvalidCommand(error) => write!(formatter, "invalid command: {error}"),
            Self::UnsupportedCommand(command) => {
                write!(formatter, "unsupported command: {command}")
            }
            Self::CommandIdConflict(command_id) => {
                write!(
                    formatter,
                    "command ID was reused with different content: {command_id}"
                )
            }
            Self::ProjectionRevisionConflict { expected, actual } => write!(
                formatter,
                "projection revision conflict: expected {expected}, actual {actual}"
            ),
            Self::TaskNotFound(task_id) => write!(formatter, "Task not found: {task_id}"),
            Self::TaskNotReady(task_id) => write!(formatter, "Task is not READY: {task_id}"),
            Self::TaskAlreadyHasRun(task_id) => {
                write!(formatter, "Task already has a Run: {task_id}")
            }
        }
    }
}

impl std::error::Error for BootstrapError {}

impl From<std::io::Error> for BootstrapError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

pub fn validate_project(
    project_root: impl AsRef<Path>,
) -> Result<ProjectValidation, BootstrapError> {
    let documents = load_task_documents(project_root.as_ref())?;
    let order = validate_task_documents(&documents)
        .map_err(|error| BootstrapError::Document(error.to_string()))?;
    Ok(ProjectValidation {
        task_count: documents.len(),
        integration_order: order
            .into_iter()
            .map(|task_id| task_id.as_str().to_owned())
            .collect(),
    })
}

pub fn rebuild_project(
    project_root: impl AsRef<Path>,
    coordinator_instance_id: &str,
) -> Result<ProjectSnapshot, BootstrapError> {
    let session = start_project(project_root, coordinator_instance_id)?;
    Ok(session.snapshot().clone())
}

pub fn start_project(
    project_root: impl AsRef<Path>,
    coordinator_instance_id: &str,
) -> Result<ProjectSession, BootstrapError> {
    let project_root = project_root.as_ref();
    let writer_lease = ProjectWriterLease::acquire(project_root, coordinator_instance_id, 1)
        .map_err(|error| BootstrapError::Coordinator(error.to_string()))?;
    let documents = load_task_documents(project_root)?;
    validate_task_documents(&documents)
        .map_err(|error| BootstrapError::Document(error.to_string()))?;

    let game_dev = project_root.join(".game-dev");
    let events_directory = game_dev.join("events");
    fs::create_dir_all(&events_directory).map_err(BootstrapError::from)?;
    let journal = EventJournal::open(events_directory.join("events.jsonl"))
        .map_err(|error| BootstrapError::Journal(error.to_string()))?;
    let mut projection = ProjectionStore::open(game_dev.join("read-model.sqlite"))
        .map_err(|error| BootstrapError::Projection(error.to_string()))?;
    projection
        .rebuild(&documents, journal.events())
        .map_err(|error| BootstrapError::Projection(error.to_string()))?;

    let snapshot = ProjectSnapshot {
        projection_revision: projection
            .projection_revision()
            .map_err(|error| BootstrapError::Projection(error.to_string()))?,
        development_board: projection
            .development_board_rows()
            .map_err(|error| BootstrapError::Projection(error.to_string()))?,
    };

    Ok(ProjectSession {
        snapshot,
        documents,
        journal,
        projection,
        _writer_lease: writer_lease,
    })
}

fn validate_command_context(context: &CommandContext) -> Result<(), BootstrapError> {
    for (field, value) in [
        ("command_id", context.command_id.as_str()),
        ("actor", context.actor.as_str()),
        ("occurred_at", context.occurred_at.as_str()),
        ("base_commit", context.base_commit.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(BootstrapError::InvalidCommand(format!(
                "{field} must not be empty"
            )));
        }
    }
    Ok(())
}

fn queued_event(
    context: &CommandContext,
    task_run: &TaskRun,
) -> Result<EventEnvelope, BootstrapError> {
    let run_id = task_run.id().as_str();
    let mut payload = BTreeMap::new();
    payload.insert("task_id".to_owned(), task_run.task_id().as_str().to_owned());
    payload.insert("run_id".to_owned(), run_id.to_owned());
    payload.insert("state".to_owned(), "QUEUED".to_owned());
    payload.insert(
        "contract_revision".to_owned(),
        task_run.contract_revision().get().to_string(),
    );
    payload.insert(
        "base_commit".to_owned(),
        task_run.base_commit().as_str().to_owned(),
    );
    let aggregate = AggregateRef::new("TaskRun", run_id)
        .map_err(|error| BootstrapError::Journal(error.to_string()))?;
    EventEnvelope::new(
        EventHeader {
            event_id: format!("EVT-{}", context.command_id),
            schema_version: SUPPORTED_SCHEMA_VERSION,
            occurred_at: context.occurred_at.clone(),
            aggregate,
            aggregate_version: 1,
            correlation_id: context.command_id.clone(),
            causation_id: None,
            actor: context.actor.clone(),
        },
        "TaskRunQueued",
        payload,
    )
    .map_err(|error| BootstrapError::Journal(error.to_string()))
}

fn load_task_documents(project_root: &Path) -> Result<Vec<TaskDocument>, BootstrapError> {
    let tasks_directory = project_root.join(".game-dev/tasks");
    let entries = fs::read_dir(&tasks_directory).map_err(BootstrapError::from)?;
    let mut paths = entries
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(BootstrapError::from)?;
    paths.retain(|path| path.extension().is_some_and(|extension| extension == "md"));
    paths.sort();
    if paths.is_empty() {
        return Err(BootstrapError::NoTaskDocuments);
    }

    paths
        .into_iter()
        .map(|path| {
            let source = fs::read_to_string(&path).map_err(BootstrapError::from)?;
            load_task_document(&source)
                .map_err(|error| BootstrapError::Document(format!("{}: {error}", path.display())))
        })
        .collect()
}
