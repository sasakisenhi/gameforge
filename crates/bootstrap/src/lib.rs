//! Shared composition root for desktop and headless coordinators.
#![allow(clippy::missing_errors_doc)]

mod coordinator;
mod events;
mod model;
mod project;

pub use coordinator::{ProjectCoordinator, UnavailableLocalVerification};
pub use model::{BootstrapError, CommandContext, ProjectSnapshot, ProjectValidation};
pub use project::{rebuild_project, start_project, validate_project};

use events::{
    cancelled_event, event_payload, execution_update_event, input_answered_event,
    input_required_event, queued_event, start_requested_event, supervisor_state_event,
};

use std::{collections::BTreeMap, fs, path::PathBuf};

use gameforge_application::{
    ApplicationCommand, LocalVerificationRequest, RunExecutionPort, RunLaunchOutcome,
    RunLaunchRequest,
};
use gameforge_domain::{
    CommitSha, ContractRevision, TaskId, TaskRun, TaskRunCommand, TaskRunEvent, TaskRunId,
    decide_task_run, evolve_task_run,
};
use gameforge_event_journal::{
    AggregateRef, EventEnvelope, EventHeader, EventJournal, SUPPORTED_SCHEMA_VERSION,
};
use gameforge_persistence::ProjectionStore;
use gameforge_project_documents::{
    TaskDocument, TaskDocumentStatus, is_test_path, load_task_document, mock_task_draft,
    render_task_markdown, validate_changed_paths, validate_task_documents,
};
use gameforge_runtime::{
    ProjectWriterLease, QueuedRun, ScheduleConfig, ScheduleSnapshot, plan_schedule,
};

pub struct ProjectSession {
    project_root: PathBuf,
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
            ApplicationCommand::AddTaskFromConversation {
                task_id,
                request,
                expected_projection_revision,
            } => self.add_task_from_conversation(
                context,
                &task_id,
                &request,
                expected_projection_revision,
            ),
            ApplicationCommand::QueueTaskRun {
                task_id,
                expected_projection_revision,
            } => self.queue_task_run(context, &task_id, expected_projection_revision),
            ApplicationCommand::CancelTaskRun {
                task_run_id,
                expected_projection_revision,
            } => self.cancel_task_run(context, &task_run_id, expected_projection_revision),
            ApplicationCommand::AnswerInputRequest {
                request_id,
                answer,
                expected_projection_revision,
            } => self.answer_input_request(
                context,
                &request_id,
                &answer,
                expected_projection_revision,
            ),
        }
    }

    fn add_task_from_conversation(
        &mut self,
        context: CommandContext,
        task_id: &str,
        request: &str,
        expected: u64,
    ) -> Result<ProjectSnapshot, BootstrapError> {
        if expected != self.snapshot.projection_revision {
            return Err(BootstrapError::ProjectionRevisionConflict {
                expected,
                actual: self.snapshot.projection_revision,
            });
        }
        if self
            .documents
            .iter()
            .any(|document| document.id().as_str() == task_id)
        {
            return Err(BootstrapError::InvalidCommand(format!(
                "Task ID already exists: {task_id}"
            )));
        }
        let draft = mock_task_draft(task_id, request);
        let source = render_task_markdown(&draft);
        let document = load_task_document(&source)
            .map_err(|error| BootstrapError::Document(error.to_string()))?;
        let path = self
            .project_root
            .join(".game-dev/tasks")
            .join(format!("{task_id}.md"));
        fs::write(path, source).map_err(BootstrapError::from)?;
        self.documents.push(document);
        self.documents
            .sort_by(|left, right| left.id().as_str().cmp(right.id().as_str()));
        validate_task_documents(&self.documents)
            .map_err(|error| BootstrapError::Document(error.to_string()))?;
        self.projection
            .rebuild(&self.documents, self.journal.events())
            .map_err(|error| BootstrapError::Projection(error.to_string()))?;
        self.refresh_snapshot()?;
        let _ = context;
        Ok(self.snapshot.clone())
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
        context: &CommandContext,
        run_id: &str,
        execution: &mut impl RunExecutionPort,
    ) -> Result<ProjectSnapshot, BootstrapError> {
        validate_command_context(context)?;
        if let Some(previous) = self.supervisor_retry(context, run_id)? {
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
        let requested = start_requested_event(context, &request, &preparation)?;
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
                (TaskRunCommand::DeferPreparation, "QUEUED", payload)
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
            context,
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

    pub fn record_input_required(
        &mut self,
        context: CommandContext,
        run_id: &str,
        request_id: &str,
        prompt: &str,
        expected_projection_revision: u64,
    ) -> Result<ProjectSnapshot, BootstrapError> {
        validate_command_context(&context)?;
        validate_required("request_id", request_id)?;
        validate_required("prompt", prompt)?;
        if let Some(previous) = self
            .journal
            .events()
            .iter()
            .find(|event| event.header().correlation_id == context.command_id)
        {
            let same_command = previous.event_type() == "TaskRunStateChanged"
                && previous.payload().get("run_id").map(String::as_str) == Some(run_id)
                && previous.payload().get("state").map(String::as_str) == Some("INPUT_REQUIRED")
                && previous.payload().get("request_id").map(String::as_str) == Some(request_id)
                && previous.payload().get("request_prompt").map(String::as_str) == Some(prompt)
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
        if self
            .snapshot
            .inbox
            .iter()
            .any(|item| item.request_id == request_id)
        {
            return Err(BootstrapError::InputRequestAlreadyExists(
                request_id.to_owned(),
            ));
        }
        let row = self
            .snapshot
            .development_board
            .iter()
            .find(|row| row.current_run_id.as_deref() == Some(run_id))
            .ok_or_else(|| BootstrapError::RunNotFound(run_id.to_owned()))?;
        if row.run_status.as_deref() != Some("AGENT_RUNNING") {
            return Err(BootstrapError::RunStateConflict {
                run_id: run_id.to_owned(),
                expected: "AGENT_RUNNING",
                actual: row.run_status.clone(),
            });
        }

        let (run, latest) = self.task_run_history(run_id)?;
        let domain_events = decide_task_run(&run, TaskRunCommand::RequireInput)
            .map_err(|error| BootstrapError::InvalidCommand(error.to_string()))?;
        if domain_events != [TaskRunEvent::InputRequired] {
            return Err(BootstrapError::InvalidCommand(
                "RequireInput must emit InputRequired".to_owned(),
            ));
        }
        let event = input_required_event(
            &context,
            &run,
            &latest,
            request_id,
            prompt,
            expected_projection_revision,
        )?;
        self.journal
            .append(&event)
            .map_err(|error| BootstrapError::Journal(error.to_string()))?;
        self.projection
            .apply_event(&event)
            .map_err(|error| BootstrapError::Projection(error.to_string()))?;
        self.refresh_snapshot()?;
        Ok(self.snapshot.clone())
    }

    pub fn record_agent_completed(
        &mut self,
        context: &CommandContext,
        run_id: &str,
        agent_session_id: &str,
        red_evidence_present: bool,
        green_evidence_present: bool,
    ) -> Result<ProjectSnapshot, BootstrapError> {
        validate_command_context(context)?;
        validate_required("agent_session_id", agent_session_id)?;
        let (run, latest) = self.task_run_history(run_id)?;
        let domain_events = decide_task_run(&run, TaskRunCommand::StartLocalChecks)
            .map_err(|error| BootstrapError::InvalidCommand(error.to_string()))?;
        if domain_events != [TaskRunEvent::LocalChecksStarted] {
            return Err(BootstrapError::InvalidCommand(
                "agent completion must start Local Checks".to_owned(),
            ));
        }
        let mut payload = BTreeMap::new();
        payload.insert("agent_session_id".to_owned(), agent_session_id.to_owned());
        payload.insert(
            "red_evidence_present".to_owned(),
            red_evidence_present.to_string(),
        );
        payload.insert(
            "green_evidence_present".to_owned(),
            green_evidence_present.to_string(),
        );
        let event = execution_update_event(context, &run, &latest, "LOCAL_CHECKING", payload)?;
        self.journal
            .append(&event)
            .map_err(|error| BootstrapError::Journal(error.to_string()))?;
        self.projection
            .apply_event(&event)
            .map_err(|error| BootstrapError::Projection(error.to_string()))?;
        self.refresh_snapshot()?;
        Ok(self.snapshot.clone())
    }

    pub fn local_verification_request(
        &self,
        run_id: &str,
    ) -> Result<LocalVerificationRequest, BootstrapError> {
        let (run, latest) = self.task_run_history(run_id)?;
        if latest.payload().get("state").map(String::as_str) != Some("LOCAL_CHECKING") {
            return Err(BootstrapError::RunStateConflict {
                run_id: run_id.to_owned(),
                expected: "LOCAL_CHECKING",
                actual: latest.payload().get("state").cloned(),
            });
        }
        let worktree_path = self
            .journal
            .events()
            .iter()
            .rev()
            .filter(|event| event.header().aggregate.aggregate_id() == run_id)
            .find_map(|event| event.payload().get("worktree_lease_id"))
            .cloned()
            .ok_or_else(|| {
                BootstrapError::Journal(format!("Task Run {run_id} has no recorded worktree lease"))
            })?;
        Ok(LocalVerificationRequest {
            task_run_id: run_id.to_owned(),
            task_id: run.task_id().as_str().to_owned(),
            base_commit: run.base_commit().as_str().to_owned(),
            worktree_path,
        })
    }

    pub fn record_local_verification_passed(
        &mut self,
        context: &CommandContext,
        run_id: &str,
        head_commit: &str,
        changed_paths: &[String],
        completed_checks: &[String],
        final_suite_passed: bool,
    ) -> Result<ProjectSnapshot, BootstrapError> {
        validate_command_context(context)?;
        validate_required("head_commit", head_commit)?;
        if completed_checks.is_empty() {
            return Err(BootstrapError::InvalidCommand(
                "completed_checks must not be empty".to_owned(),
            ));
        }
        let (run, latest) = self.task_run_history(run_id)?;
        if latest.payload().get("state").map(String::as_str) != Some("LOCAL_CHECKING") {
            return Err(BootstrapError::RunStateConflict {
                run_id: run_id.to_owned(),
                expected: "LOCAL_CHECKING",
                actual: latest.payload().get("state").cloned(),
            });
        }
        let document = self.task_document_for_run(&run)?;
        if let Err(error) = validate_changed_paths(document, changed_paths) {
            return self.fail_local_verification(
                context,
                &run,
                &latest,
                &error.to_string(),
                Some("SCOPE_VIOLATION"),
            );
        }

        let behavior_changed = changed_paths
            .iter()
            .any(|path| !is_test_path(document, path));
        let red_evidence_present = boolean_payload(&latest, "red_evidence_present")?;
        let agent_green_evidence = boolean_payload(&latest, "green_evidence_present")?;
        let green_evidence_present =
            agent_green_evidence || (red_evidence_present && final_suite_passed);
        let head_commit = CommitSha::new(head_commit)
            .map_err(|error| BootstrapError::InvalidCommand(error.to_string()))?;
        let command = TaskRunCommand::CompleteLocalChecks {
            required_checks_passed: true,
            scope_check_passed: true,
            final_suite_passed,
            behavior_changed,
            red_evidence_present,
            green_evidence_present,
            head_commit,
        };
        let domain_events = match decide_task_run(&run, command) {
            Ok(events) => events,
            Err(error) => {
                let health_flag = match error {
                    gameforge_domain::DomainError::MissingTddEvidence => {
                        Some("TDD_SEQUENCE_VIOLATION")
                    }
                    _ => None,
                };
                return self.fail_local_verification(
                    context,
                    &run,
                    &latest,
                    &error.to_string(),
                    health_flag,
                );
            }
        };
        let [TaskRunEvent::Succeeded { head_commit }] = domain_events.as_slice() else {
            return Err(BootstrapError::InvalidCommand(
                "Local Verification must emit Succeeded".to_owned(),
            ));
        };

        let mut payload = BTreeMap::new();
        payload.insert("head_commit".to_owned(), head_commit.as_str().to_owned());
        payload.insert("behavior_changed".to_owned(), behavior_changed.to_string());
        payload.insert(
            "red_evidence_present".to_owned(),
            red_evidence_present.to_string(),
        );
        payload.insert(
            "green_evidence_present".to_owned(),
            green_evidence_present.to_string(),
        );
        payload.insert(
            "changed_path_count".to_owned(),
            changed_paths.len().to_string(),
        );
        for (index, path) in changed_paths.iter().enumerate() {
            payload.insert(format!("changed_path_{index}"), path.clone());
        }
        payload.insert(
            "completed_check_count".to_owned(),
            completed_checks.len().to_string(),
        );
        for (index, check) in completed_checks.iter().enumerate() {
            payload.insert(format!("completed_check_{index}"), check.clone());
        }
        let event = execution_update_event(context, &run, &latest, "SUCCEEDED", payload)?;
        self.append_and_project(&event)?;
        Ok(self.snapshot.clone())
    }

    pub fn record_local_verification_failed(
        &mut self,
        context: &CommandContext,
        run_id: &str,
        detail: &str,
    ) -> Result<ProjectSnapshot, BootstrapError> {
        validate_command_context(context)?;
        validate_required("failure_detail", detail)?;
        let (run, latest) = self.task_run_history(run_id)?;
        if latest.payload().get("state").map(String::as_str) != Some("LOCAL_CHECKING") {
            return Err(BootstrapError::RunStateConflict {
                run_id: run_id.to_owned(),
                expected: "LOCAL_CHECKING",
                actual: latest.payload().get("state").cloned(),
            });
        }
        self.fail_local_verification(context, &run, &latest, detail, None)
    }

    pub fn record_agent_failed(
        &mut self,
        context: &CommandContext,
        run_id: &str,
        detail: &str,
    ) -> Result<ProjectSnapshot, BootstrapError> {
        validate_command_context(context)?;
        validate_required("failure_detail", detail)?;
        let (run, latest) = self.task_run_history(run_id)?;
        let domain_events = decide_task_run(
            &run,
            TaskRunCommand::Fail {
                reason: detail.to_owned(),
            },
        )
        .map_err(|error| BootstrapError::InvalidCommand(error.to_string()))?;
        if domain_events
            != [TaskRunEvent::Failed {
                reason: detail.to_owned(),
            }]
        {
            return Err(BootstrapError::InvalidCommand(
                "agent failure must fail the Task Run".to_owned(),
            ));
        }
        let mut payload = BTreeMap::new();
        payload.insert("failure_detail".to_owned(), detail.to_owned());
        let event = execution_update_event(context, &run, &latest, "FAILED", payload)?;
        self.journal
            .append(&event)
            .map_err(|error| BootstrapError::Journal(error.to_string()))?;
        self.projection
            .apply_event(&event)
            .map_err(|error| BootstrapError::Projection(error.to_string()))?;
        self.refresh_snapshot()?;
        Ok(self.snapshot.clone())
    }

    fn task_document_for_run(&self, run: &TaskRun) -> Result<&TaskDocument, BootstrapError> {
        self.documents
            .iter()
            .find(|document| {
                document.id() == run.task_id()
                    && document.contract_revision() == run.contract_revision()
            })
            .ok_or_else(|| {
                BootstrapError::Document(format!(
                    "Task Contract revision {} for {} is not loaded",
                    run.contract_revision().get(),
                    run.task_id()
                ))
            })
    }

    fn fail_local_verification(
        &mut self,
        context: &CommandContext,
        run: &TaskRun,
        latest: &EventEnvelope,
        detail: &str,
        health_flag: Option<&str>,
    ) -> Result<ProjectSnapshot, BootstrapError> {
        let domain_events = decide_task_run(
            run,
            TaskRunCommand::Fail {
                reason: detail.to_owned(),
            },
        )
        .map_err(|error| BootstrapError::InvalidCommand(error.to_string()))?;
        if domain_events
            != [TaskRunEvent::Failed {
                reason: detail.to_owned(),
            }]
        {
            return Err(BootstrapError::InvalidCommand(
                "Local Verification failure must fail the Task Run".to_owned(),
            ));
        }
        let mut payload = BTreeMap::new();
        payload.insert("failure_detail".to_owned(), detail.to_owned());
        payload.insert("failure_stage".to_owned(), "LOCAL_VERIFICATION".to_owned());
        if let Some(flag) = health_flag {
            payload.insert("health_flag".to_owned(), flag.to_owned());
        }
        let event = execution_update_event(context, run, latest, "FAILED", payload)?;
        self.append_and_project(&event)?;
        Ok(self.snapshot.clone())
    }

    fn append_and_project(&mut self, event: &EventEnvelope) -> Result<(), BootstrapError> {
        self.journal
            .append(event)
            .map_err(|error| BootstrapError::Journal(error.to_string()))?;
        self.projection
            .apply_event(event)
            .map_err(|error| BootstrapError::Projection(error.to_string()))?;
        self.refresh_snapshot()
    }

    pub(crate) fn preparing_run_ids(&self) -> Vec<String> {
        self.snapshot
            .development_board
            .iter()
            .filter(|row| row.run_status.as_deref() == Some("PREPARING"))
            .filter_map(|row| row.current_run_id.clone())
            .collect()
    }

    pub(crate) fn local_checking_run_ids(&self) -> Vec<String> {
        self.snapshot
            .development_board
            .iter()
            .filter(|row| row.run_status.as_deref() == Some("LOCAL_CHECKING"))
            .filter_map(|row| row.current_run_id.clone())
            .collect()
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
        if row.current_run_id.is_some() && row.run_status.as_deref() != Some("FAILED") {
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

    fn cancel_task_run(
        &mut self,
        context: CommandContext,
        run_id: &str,
        expected_projection_revision: u64,
    ) -> Result<ProjectSnapshot, BootstrapError> {
        if let Some(previous) = self
            .journal
            .events()
            .iter()
            .find(|event| event.header().correlation_id == context.command_id)
        {
            let same_command = previous.event_type() == "TaskRunStateChanged"
                && previous.payload().get("run_id").map(String::as_str) == Some(run_id)
                && previous.payload().get("state").map(String::as_str) == Some("CANCELLED")
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
        self.snapshot
            .development_board
            .iter()
            .find(|row| row.current_run_id.as_deref() == Some(run_id))
            .ok_or_else(|| BootstrapError::RunNotFound(run_id.to_owned()))?;

        let (run, latest) = self.task_run_history(run_id)?;
        let domain_events = decide_task_run(&run, TaskRunCommand::Cancel)
            .map_err(|error| BootstrapError::InvalidCommand(error.to_string()))?;
        if domain_events != [TaskRunEvent::Cancelled] {
            return Err(BootstrapError::InvalidCommand(
                "Cancel must emit Cancelled".to_owned(),
            ));
        }
        let event = cancelled_event(&context, &run, &latest, expected_projection_revision)?;
        self.journal
            .append(&event)
            .map_err(|error| BootstrapError::Journal(error.to_string()))?;
        self.projection
            .apply_event(&event)
            .map_err(|error| BootstrapError::Projection(error.to_string()))?;
        self.refresh_snapshot()?;
        Ok(self.snapshot.clone())
    }

    fn answer_input_request(
        &mut self,
        context: CommandContext,
        request_id: &str,
        answer: &str,
        expected_projection_revision: u64,
    ) -> Result<ProjectSnapshot, BootstrapError> {
        validate_required("request_id", request_id)?;
        validate_required("answer", answer)?;
        if let Some(previous) = self
            .journal
            .events()
            .iter()
            .find(|event| event.header().correlation_id == context.command_id)
        {
            let same_command = previous.event_type() == "TaskRunStateChanged"
                && previous.payload().get("state").map(String::as_str) == Some("AGENT_RUNNING")
                && previous
                    .payload()
                    .get("resolved_request_id")
                    .map(String::as_str)
                    == Some(request_id)
                && previous.payload().get("input_answer").map(String::as_str) == Some(answer)
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
        let request = self
            .snapshot
            .inbox
            .iter()
            .find(|item| item.request_id == request_id)
            .ok_or_else(|| BootstrapError::InputRequestNotFound(request_id.to_owned()))?;
        if request.status != "PENDING" {
            return Err(BootstrapError::InputRequestNotPending {
                request_id: request_id.to_owned(),
                actual: request.status.clone(),
            });
        }
        let run_id = request.task_run_id.clone();
        let row = self
            .snapshot
            .development_board
            .iter()
            .find(|row| row.current_run_id.as_deref() == Some(run_id.as_str()))
            .ok_or_else(|| BootstrapError::RunNotFound(run_id.clone()))?;
        if row.run_status.as_deref() != Some("INPUT_REQUIRED") {
            return Err(BootstrapError::RunStateConflict {
                run_id,
                expected: "INPUT_REQUIRED",
                actual: row.run_status.clone(),
            });
        }

        let (run, latest) = self.task_run_history(&request.task_run_id)?;
        let domain_events = decide_task_run(&run, TaskRunCommand::ResumeAfterInput)
            .map_err(|error| BootstrapError::InvalidCommand(error.to_string()))?;
        if domain_events != [TaskRunEvent::InputProvided] {
            return Err(BootstrapError::InvalidCommand(
                "ResumeAfterInput must emit InputProvided".to_owned(),
            ));
        }
        let event = input_answered_event(
            &context,
            &run,
            &latest,
            request_id,
            answer,
            expected_projection_revision,
        )?;
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
            inbox: self
                .projection
                .inbox_rows()
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
                Some("PREPARING" | "AGENT_RUNNING" | "INPUT_REQUIRED" | "DECISION_REQUIRED") => {
                    running.push(run_id.to_owned());
                }
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

    fn prepared_run(&self, run_id: &str) -> Result<(TaskRun, EventEnvelope), BootstrapError> {
        let (run, latest) = self.task_run_history(run_id)?;
        let actual = latest.payload().get("state").cloned();
        if actual.as_deref() != Some("PREPARING") {
            return Err(BootstrapError::RunNotPreparing {
                run_id: run_id.to_owned(),
                actual,
            });
        }
        Ok((run, latest))
    }

    fn task_run_history(&self, run_id: &str) -> Result<(TaskRun, EventEnvelope), BootstrapError> {
        let queued = self.queued_event_for_run(run_id)?;
        let task_id = event_payload(queued, "task_id")?;
        let contract_revision = event_payload(queued, "contract_revision")?
            .parse::<u64>()
            .map_err(|_| BootstrapError::Journal("invalid contract_revision".to_owned()))?;
        let mut run = TaskRun::new(
            TaskRunId::new(run_id).map_err(|error| BootstrapError::Journal(error.to_string()))?,
            TaskId::new(task_id).map_err(|error| BootstrapError::Journal(error.to_string()))?,
            ContractRevision::new(contract_revision)
                .map_err(|error| BootstrapError::Journal(error.to_string()))?,
            CommitSha::new(event_payload(queued, "base_commit")?)
                .map_err(|error| BootstrapError::Journal(error.to_string()))?,
        );
        let mut latest = queued.clone();
        for event in self.journal.events().iter().filter(|event| {
            event.event_type() == "TaskRunStateChanged"
                && event.header().aggregate.aggregate_id() == run_id
        }) {
            let domain_event = match event.payload().get("state").map(String::as_str) {
                Some("PREPARING") => TaskRunEvent::PreparationStarted,
                Some("QUEUED") => TaskRunEvent::PreparationDeferred,
                Some("AGENT_RUNNING")
                    if event.payload().get("resolution_kind").map(String::as_str)
                        == Some("INPUT_ANSWERED") =>
                {
                    TaskRunEvent::InputProvided
                }
                Some("AGENT_RUNNING") => TaskRunEvent::AgentStarted,
                Some("INPUT_REQUIRED") => TaskRunEvent::InputRequired,
                Some("LOCAL_CHECKING") => TaskRunEvent::LocalChecksStarted,
                Some("SUCCEEDED") => TaskRunEvent::Succeeded {
                    head_commit: CommitSha::new(event_payload(event, "head_commit")?)
                        .map_err(|error| BootstrapError::Journal(error.to_string()))?,
                },
                Some("FAILED") => TaskRunEvent::Failed {
                    reason: event
                        .payload()
                        .get("failure_detail")
                        .cloned()
                        .unwrap_or_else(|| "agent execution failed".to_owned()),
                },
                Some("CANCELLED") => TaskRunEvent::Cancelled,
                Some(state) => {
                    return Err(BootstrapError::Journal(format!(
                        "unsupported reconstructed TaskRun state {state} for {run_id}"
                    )));
                }
                None => {
                    return Err(BootstrapError::Journal(format!(
                        "TaskRunStateChanged event is missing state for {run_id}"
                    )));
                }
            };
            run = evolve_task_run(run, &domain_event);
            latest = event.clone();
        }
        Ok((run, latest))
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
        let (run, latest) = self.task_run_history(run_id)?;
        let domain_events = decide_task_run(&run, TaskRunCommand::Prepare)
            .map_err(|error| BootstrapError::InvalidCommand(error.to_string()))?;
        if domain_events != [TaskRunEvent::PreparationStarted] {
            return Err(BootstrapError::InvalidCommand(
                "Prepare must emit PreparationStarted".to_owned(),
            ));
        }

        let mut payload = BTreeMap::new();
        payload.insert("task_id".to_owned(), run.task_id().as_str().to_owned());
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
                aggregate_version: latest.header().aggregate_version + 1,
                correlation_id: context.command_id.clone(),
                causation_id: Some(latest.header().event_id.clone()),
                actor: context.actor.clone(),
            },
            "TaskRunStateChanged",
            payload,
        )
        .map_err(|error| BootstrapError::Journal(error.to_string()))
    }
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

fn validate_required(field: &'static str, value: &str) -> Result<(), BootstrapError> {
    if value.trim().is_empty() {
        Err(BootstrapError::InvalidCommand(format!(
            "{field} must not be empty"
        )))
    } else {
        Ok(())
    }
}

fn boolean_payload(event: &EventEnvelope, field: &'static str) -> Result<bool, BootstrapError> {
    event.payload().get(field).map_or(Ok(false), |value| {
        value
            .parse::<bool>()
            .map_err(|_| BootstrapError::Journal(format!("invalid boolean payload field {field}")))
    })
}
