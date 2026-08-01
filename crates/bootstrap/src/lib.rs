//! Shared composition root for desktop and headless coordinators.
#![allow(clippy::missing_errors_doc)]

use std::{collections::BTreeMap, fmt, fs, path::Path};

use gameforge_application::{
    ApplicationCommand, RunExecutionPort, RunLaunchOutcome, RunLaunchRequest,
};
use gameforge_domain::{
    CommitSha, ContractRevision, TaskId, TaskRun, TaskRunCommand, TaskRunEvent, TaskRunId,
    decide_task_run, evolve_task_run,
};
use gameforge_event_journal::{
    AggregateRef, EventEnvelope, EventHeader, EventJournal, SUPPORTED_SCHEMA_VERSION,
};
use gameforge_persistence::{DevelopmentBoardRow, ProjectionStore};
use gameforge_project_documents::{
    TaskDocument, TaskDocumentStatus, load_task_document, validate_task_documents,
};
use gameforge_runtime::{
    ProjectWriterLease, QueuedRun, ScheduleConfig, ScheduleSnapshot, plan_schedule,
};

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

    pub fn run_scheduler(
        &mut self,
        context: CommandContext,
        config: ScheduleConfig,
    ) -> Result<ProjectSnapshot, BootstrapError> {
        validate_command_context(&context)?;
        let previous = self
            .journal
            .events()
            .iter()
            .filter(|event| event.header().correlation_id == context.command_id)
            .collect::<Vec<_>>();
        if !previous.is_empty() {
            let same_command = previous.iter().all(|event| {
                event.event_type() == "TaskRunStateChanged"
                    && event.payload().get("state").map(String::as_str) == Some("PREPARING")
                    && event
                        .payload()
                        .get("max_concurrent_task_runs")
                        .and_then(|maximum| maximum.parse::<usize>().ok())
                        == Some(config.max_concurrent_task_runs)
                    && event.header().actor == context.actor
                    && event.header().occurred_at == context.occurred_at
            });
            return if same_command {
                Ok(self.snapshot.clone())
            } else {
                Err(BootstrapError::CommandIdConflict(context.command_id))
            };
        }

        let schedule_snapshot = self.schedule_snapshot()?;
        let plan = plan_schedule(&schedule_snapshot, &config);
        for run_id in plan.start {
            let event = self.preparation_event(&context, config, &run_id)?;
            self.journal
                .append(&event)
                .map_err(|error| BootstrapError::Journal(error.to_string()))?;
            self.projection
                .apply_event(&event)
                .map_err(|error| BootstrapError::Projection(error.to_string()))?;
        }
        self.refresh_snapshot()?;
        Ok(self.snapshot.clone())
    }

    pub fn run_supervisor(
        &mut self,
        context: CommandContext,
        run_id: &str,
        execution: &mut impl RunExecutionPort,
    ) -> Result<ProjectSnapshot, BootstrapError> {
        validate_command_context(&context)?;
        if let Some(previous) = self.supervisor_retry(&context, run_id)? {
            return Ok(previous);
        }

        let row = self
            .snapshot
            .development_board
            .iter()
            .find(|row| row.current_run_id.as_deref() == Some(run_id))
            .ok_or_else(|| BootstrapError::RunNotFound(run_id.to_owned()))?;
        if row.run_status.as_deref() != Some("PREPARING") {
            return Err(BootstrapError::RunNotPreparing {
                run_id: run_id.to_owned(),
                actual: row.run_status.clone(),
            });
        }

        let (prepared_run, preparation) = self.prepared_run(run_id)?;
        let request = RunLaunchRequest {
            task_run_id: run_id.to_owned(),
            task_id: prepared_run.task_id().as_str().to_owned(),
            contract_revision: prepared_run.contract_revision().get(),
            base_commit: prepared_run.base_commit().as_str().to_owned(),
        };
        let requested = start_requested_event(&context, &request, &preparation)?;
        self.journal
            .append(&requested)
            .map_err(|error| BootstrapError::Journal(error.to_string()))?;
        self.projection
            .apply_event(&requested)
            .map_err(|error| BootstrapError::Projection(error.to_string()))?;

        let outcome = execution.start_run(&request);
        let (command, state, outcome_payload) = match outcome {
            RunLaunchOutcome::Started(started) => {
                let mut payload = BTreeMap::new();
                payload.insert(
                    "resource_lease_id".to_owned(),
                    started.resource_lease_id().to_owned(),
                );
                payload.insert(
                    "worktree_lease_id".to_owned(),
                    started.worktree_lease_id().to_owned(),
                );
                payload.insert(
                    "agent_session_id".to_owned(),
                    started.agent_session_id().to_owned(),
                );
                (
                    TaskRunCommand::StartAgent {
                        has_resource_lease: true,
                        has_worktree_lease: true,
                    },
                    "AGENT_RUNNING",
                    payload,
                )
            }
            RunLaunchOutcome::Deferred { reason, detail } => {
                let mut payload = BTreeMap::new();
                payload.insert("deferral_reason".to_owned(), reason.as_str().to_owned());
                payload.insert("deferral_detail".to_owned(), detail);
                (
                    TaskRunCommand::DeferPreparation,
                    "QUEUED",
                    payload,
                )
            }
        };
        let domain_events = decide_task_run(&prepared_run, command)
            .map_err(|error| BootstrapError::InvalidCommand(error.to_string()))?;
        let expected_event = if state == "AGENT_RUNNING" {
            TaskRunEvent::AgentStarted
        } else {
            TaskRunEvent::PreparationDeferred
        };
        if domain_events != [expected_event] {
            return Err(BootstrapError::InvalidCommand(
                "Supervisor emitted an unexpected TaskRun event".to_owned(),
            ));
        }

        let state_changed = supervisor_state_event(
            &context,
            &request,
            &preparation,
            &requested,
            state,
            outcome_payload,
        )?;
        self.journal
            .append(&state_changed)
            .map_err(|error| BootstrapError::Journal(error.to_string()))?;
        self.projection
            .apply_event(&state_changed)
            .map_err(|error| BootstrapError::Projection(error.to_string()))?;
        self.refresh_snapshot()?;
        Ok(self.snapshot.clone())
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
                && previous.payload().get("task_id").map(String::as_str) == Some(task_id)
                && previous.payload().get("base_commit").map(String::as_str)
                    == Some(context.base_commit.as_str())
                && previous
                    .payload()
                    .get("expected_projection_revision")
                    .and_then(|revision| revision.parse::<u64>().ok())
                    == Some(expected_projection_revision)
                && previous.header().actor == context.actor
                && previous.header().occurred_at == context.occurred_at;
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
        let event = queued_event(&context, &task_run, expected_projection_revision)?;

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

    fn schedule_snapshot(&self) -> Result<ScheduleSnapshot, BootstrapError> {
        let mut queued = Vec::new();
        let mut running = Vec::new();
        for row in &self.snapshot.development_board {
            let Some(run_id) = row.current_run_id.as_deref() else {
                continue;
            };
            match row.run_status.as_deref() {
                Some("QUEUED") => {
                    let queued_event = self.queued_event_for_run(run_id)?;
                    queued.push(QueuedRun {
                        task_run_id: run_id.to_owned(),
                        priority: 0,
                        queued_at: queued_event.header().occurred_at.clone(),
                        start_blocker: None,
                    });
                }
                Some(
                    "PREPARING" | "AGENT_RUNNING" | "INPUT_REQUIRED" | "DECISION_REQUIRED"
                    | "LOCAL_CHECKING",
                ) => running.push(run_id.to_owned()),
                _ => {}
            }
        }
        Ok(ScheduleSnapshot { queued, running })
    }

    fn queued_event_for_run(&self, run_id: &str) -> Result<&EventEnvelope, BootstrapError> {
        self.journal
            .events()
            .iter()
            .find(|event| {
                event.event_type() == "TaskRunQueued"
                    && event.header().aggregate.aggregate_id() == run_id
            })
            .ok_or_else(|| {
                BootstrapError::Journal(format!("TaskRunQueued event not found for {run_id}"))
            })
    }

    fn prepared_run(
        &self,
        run_id: &str,
    ) -> Result<(TaskRun, EventEnvelope), BootstrapError> {
        let queued = self.queued_event_for_run(run_id)?;
        let task_id = event_payload(queued, "task_id")?;
        let contract_revision = event_payload(queued, "contract_revision")?
            .parse::<u64>()
            .map_err(|_| BootstrapError::Journal("invalid contract_revision".to_owned()))?;
        let run = TaskRun::new(
            TaskRunId::new(run_id).map_err(|error| BootstrapError::Journal(error.to_string()))?,
            TaskId::new(task_id).map_err(|error| BootstrapError::Journal(error.to_string()))?,
            ContractRevision::new(contract_revision)
                .map_err(|error| BootstrapError::Journal(error.to_string()))?,
            CommitSha::new(event_payload(queued, "base_commit")?)
                .map_err(|error| BootstrapError::Journal(error.to_string()))?,
        );
        let preparation = self
            .journal
            .events()
            .iter()
            .find(|event| {
                event.event_type() == "TaskRunStateChanged"
                    && event.header().aggregate.aggregate_id() == run_id
                    && event.payload().get("state").map(String::as_str) == Some("PREPARING")
            })
            .cloned()
            .ok_or_else(|| {
                BootstrapError::Journal(format!(
                    "PREPARING TaskRunStateChanged event not found for {run_id}"
                ))
            })?;
        Ok((
            evolve_task_run(run, &TaskRunEvent::PreparationStarted),
            preparation,
        ))
    }

    fn supervisor_retry(
        &self,
        context: &CommandContext,
        run_id: &str,
    ) -> Result<Option<ProjectSnapshot>, BootstrapError> {
        let previous = self
            .journal
            .events()
            .iter()
            .filter(|event| event.header().correlation_id == context.command_id)
            .collect::<Vec<_>>();
        if previous.is_empty() {
            return Ok(None);
        }
        let same_request = previous.iter().any(|event| {
            event.event_type() == "TaskRunStartRequested"
                && event.payload().get("run_id").map(String::as_str) == Some(run_id)
                && event.payload().get("base_commit").map(String::as_str)
                    == Some(context.base_commit.as_str())
                && event.header().actor == context.actor
                && event.header().occurred_at == context.occurred_at
        });
        if !same_request {
            return Err(BootstrapError::CommandIdConflict(
                context.command_id.clone(),
            ));
        }
        let completed = previous.iter().any(|event| {
            event.event_type() == "TaskRunStateChanged"
                && event.header().aggregate.aggregate_id() == run_id
                && matches!(
                    event.payload().get("state").map(String::as_str),
                    Some("AGENT_RUNNING" | "QUEUED")
                )
        });
        if completed {
            Ok(Some(self.snapshot.clone()))
        } else {
            Err(BootstrapError::OperationOutcomeUnknown(
                context.command_id.clone(),
            ))
        }
    }

    fn preparation_event(
        &self,
        context: &CommandContext,
        config: ScheduleConfig,
        run_id: &str,
    ) -> Result<EventEnvelope, BootstrapError> {
        let queued = self.queued_event_for_run(run_id)?;
        let task_id = event_payload(queued, "task_id")?;
        let contract_revision = event_payload(queued, "contract_revision")?
            .parse::<u64>()
            .map_err(|_| BootstrapError::Journal("invalid contract_revision".to_owned()))?;
        let run = TaskRun::new(
            TaskRunId::new(run_id).map_err(|error| BootstrapError::Journal(error.to_string()))?,
            TaskId::new(task_id).map_err(|error| BootstrapError::Journal(error.to_string()))?,
            ContractRevision::new(contract_revision)
                .map_err(|error| BootstrapError::Journal(error.to_string()))?,
            CommitSha::new(event_payload(queued, "base_commit")?)
                .map_err(|error| BootstrapError::Journal(error.to_string()))?,
        );
        let domain_events = decide_task_run(&run, TaskRunCommand::Prepare)
            .map_err(|error| BootstrapError::InvalidCommand(error.to_string()))?;
        if domain_events != [TaskRunEvent::PreparationStarted] {
            return Err(BootstrapError::InvalidCommand(
                "Prepare must emit PreparationStarted".to_owned(),
            ));
        }

        let mut payload = BTreeMap::new();
        payload.insert("task_id".to_owned(), task_id.to_owned());
        payload.insert("run_id".to_owned(), run_id.to_owned());
        payload.insert("state".to_owned(), "PREPARING".to_owned());
        payload.insert(
            "max_concurrent_task_runs".to_owned(),
            config.max_concurrent_task_runs.to_string(),
        );
        let aggregate = AggregateRef::new("TaskRun", run_id)
            .map_err(|error| BootstrapError::Journal(error.to_string()))?;
        EventEnvelope::new(
            EventHeader {
                event_id: format!("EVT-{}-{run_id}", context.command_id),
                schema_version: SUPPORTED_SCHEMA_VERSION,
                occurred_at: context.occurred_at.clone(),
                aggregate,
                aggregate_version: queued.header().aggregate_version + 1,
                correlation_id: context.command_id.clone(),
                causation_id: Some(queued.header().event_id.clone()),
                actor: context.actor.clone(),
            },
            "TaskRunStateChanged",
            payload,
        )
        .map_err(|error| BootstrapError::Journal(error.to_string()))
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
    RunNotFound(String),
    RunNotPreparing {
        run_id: String,
        actual: Option<String>,
    },
    OperationOutcomeUnknown(String),
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
    expected_projection_revision: u64,
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
    payload.insert(
        "expected_projection_revision".to_owned(),
        expected_projection_revision.to_string(),
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

fn start_requested_event(
    context: &CommandContext,
    request: &RunLaunchRequest,
    preparation: &EventEnvelope,
) -> Result<EventEnvelope, BootstrapError> {
    let mut payload = BTreeMap::new();
    payload.insert("task_id".to_owned(), request.task_id.clone());
    payload.insert("run_id".to_owned(), request.task_run_id.clone());
    payload.insert(
        "contract_revision".to_owned(),
        request.contract_revision.to_string(),
    );
    payload.insert("base_commit".to_owned(), request.base_commit.clone());
    let aggregate = AggregateRef::new("Operation", format!("OP-{}-START", context.command_id))
        .map_err(|error| BootstrapError::Journal(error.to_string()))?;
    EventEnvelope::new(
        EventHeader {
            event_id: format!("EVT-{}-START-REQUESTED", context.command_id),
            schema_version: SUPPORTED_SCHEMA_VERSION,
            occurred_at: context.occurred_at.clone(),
            aggregate,
            aggregate_version: 1,
            correlation_id: context.command_id.clone(),
            causation_id: Some(preparation.header().event_id.clone()),
            actor: context.actor.clone(),
        },
        "TaskRunStartRequested",
        payload,
    )
    .map_err(|error| BootstrapError::Journal(error.to_string()))
}

fn supervisor_state_event(
    context: &CommandContext,
    request: &RunLaunchRequest,
    preparation: &EventEnvelope,
    requested: &EventEnvelope,
    state: &str,
    mut outcome_payload: BTreeMap<String, String>,
) -> Result<EventEnvelope, BootstrapError> {
    outcome_payload.insert("task_id".to_owned(), request.task_id.clone());
    outcome_payload.insert("run_id".to_owned(), request.task_run_id.clone());
    outcome_payload.insert("state".to_owned(), state.to_owned());
    let aggregate = AggregateRef::new("TaskRun", &request.task_run_id)
        .map_err(|error| BootstrapError::Journal(error.to_string()))?;
    EventEnvelope::new(
        EventHeader {
            event_id: format!("EVT-{}-{}", context.command_id, state),
            schema_version: SUPPORTED_SCHEMA_VERSION,
            occurred_at: context.occurred_at.clone(),
            aggregate,
            aggregate_version: preparation.header().aggregate_version + 1,
            correlation_id: context.command_id.clone(),
            causation_id: Some(requested.header().event_id.clone()),
            actor: context.actor.clone(),
        },
        "TaskRunStateChanged",
        outcome_payload,
    )
    .map_err(|error| BootstrapError::Journal(error.to_string()))
}

fn event_payload<'a>(
    event: &'a EventEnvelope,
    field: &'static str,
) -> Result<&'a str, BootstrapError> {
    event
        .payload()
        .get(field)
        .map(String::as_str)
        .ok_or_else(|| {
            BootstrapError::Journal(format!(
                "{} event is missing payload field {field}",
                event.event_type()
            ))
        })
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
