use std::collections::BTreeSet;

use crate::{CommitSha, ContractRevision, DomainError, TaskId, TaskRunId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskRunStatus {
    Queued,
    Preparing,
    AgentRunning,
    InputRequired,
    DecisionRequired,
    LocalChecking,
    Succeeded,
    Failed,
    Cancelled,
}

impl TaskRunStatus {
    const fn name(self) -> &'static str {
        match self {
            Self::Queued => "QUEUED",
            Self::Preparing => "PREPARING",
            Self::AgentRunning => "AGENT_RUNNING",
            Self::InputRequired => "INPUT_REQUIRED",
            Self::DecisionRequired => "DECISION_REQUIRED",
            Self::LocalChecking => "LOCAL_CHECKING",
            Self::Succeeded => "SUCCEEDED",
            Self::Failed => "FAILED",
            Self::Cancelled => "CANCELLED",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum HealthFlag {
    Stale,
    ScopeViolation,
    ConflictRisk,
    TddSequenceViolation,
}

impl HealthFlag {
    const fn blocks_integration(self) -> bool {
        matches!(
            self,
            Self::Stale | Self::ScopeViolation | Self::ConflictRisk | Self::TddSequenceViolation
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRun {
    id: TaskRunId,
    task_id: TaskId,
    contract_revision: ContractRevision,
    base_commit: CommitSha,
    head_commit: Option<CommitSha>,
    status: TaskRunStatus,
    health_flags: BTreeSet<HealthFlag>,
}

impl TaskRun {
    #[must_use]
    pub fn new(
        id: TaskRunId,
        task_id: TaskId,
        contract_revision: ContractRevision,
        base_commit: CommitSha,
    ) -> Self {
        Self {
            id,
            task_id,
            contract_revision,
            base_commit,
            head_commit: None,
            status: TaskRunStatus::Queued,
            health_flags: BTreeSet::new(),
        }
    }

    #[must_use]
    pub fn id(&self) -> &TaskRunId {
        &self.id
    }

    #[must_use]
    pub fn task_id(&self) -> &TaskId {
        &self.task_id
    }

    #[must_use]
    pub const fn contract_revision(&self) -> ContractRevision {
        self.contract_revision
    }

    #[must_use]
    pub fn base_commit(&self) -> &CommitSha {
        &self.base_commit
    }

    #[must_use]
    pub fn head_commit(&self) -> Option<&CommitSha> {
        self.head_commit.as_ref()
    }

    #[must_use]
    pub const fn status(&self) -> TaskRunStatus {
        self.status
    }

    #[must_use]
    pub fn with_health_flag(mut self, flag: HealthFlag) -> Self {
        self.health_flags.insert(flag);
        self
    }

    #[must_use]
    pub fn is_integrable(&self) -> bool {
        self.status == TaskRunStatus::Succeeded
            && !self
                .health_flags
                .iter()
                .any(|flag| flag.blocks_integration())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskRunCommand {
    Prepare,
    DeferPreparation,
    StartAgent {
        has_resource_lease: bool,
        has_worktree_lease: bool,
    },
    RequireInput,
    ResumeAfterInput,
    RequireDecision,
    ResumeAfterDecision,
    StartLocalChecks,
    CompleteLocalChecks {
        required_checks_passed: bool,
        scope_check_passed: bool,
        final_suite_passed: bool,
        behavior_changed: bool,
        red_evidence_present: bool,
        green_evidence_present: bool,
        head_commit: CommitSha,
    },
    Fail {
        reason: String,
    },
    Cancel,
}

impl TaskRunCommand {
    const fn name(&self) -> &'static str {
        match self {
            Self::Prepare => "Prepare",
            Self::DeferPreparation => "DeferPreparation",
            Self::StartAgent { .. } => "StartAgent",
            Self::RequireInput => "RequireInput",
            Self::ResumeAfterInput => "ResumeAfterInput",
            Self::RequireDecision => "RequireDecision",
            Self::ResumeAfterDecision => "ResumeAfterDecision",
            Self::StartLocalChecks => "StartLocalChecks",
            Self::CompleteLocalChecks { .. } => "CompleteLocalChecks",
            Self::Fail { .. } => "Fail",
            Self::Cancel => "Cancel",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskRunEvent {
    PreparationStarted,
    PreparationDeferred,
    AgentStarted,
    InputRequired,
    InputProvided,
    DecisionRequired,
    DecisionProvided,
    LocalChecksStarted,
    Succeeded { head_commit: CommitSha },
    Failed { reason: String },
    Cancelled,
}

pub fn decide_task_run(
    run: &TaskRun,
    command: TaskRunCommand,
) -> Result<Vec<TaskRunEvent>, DomainError> {
    let event = match (&run.status, &command) {
        (TaskRunStatus::Queued, TaskRunCommand::Prepare) => TaskRunEvent::PreparationStarted,
        (TaskRunStatus::Preparing, TaskRunCommand::DeferPreparation) => {
            TaskRunEvent::PreparationDeferred
        }
        (
            TaskRunStatus::Preparing,
            TaskRunCommand::StartAgent {
                has_resource_lease,
                has_worktree_lease,
            },
        ) => {
            if !has_resource_lease || !has_worktree_lease {
                return Err(DomainError::MissingLease);
            }
            TaskRunEvent::AgentStarted
        }
        (TaskRunStatus::AgentRunning, TaskRunCommand::RequireInput) => TaskRunEvent::InputRequired,
        (TaskRunStatus::InputRequired, TaskRunCommand::ResumeAfterInput) => {
            TaskRunEvent::InputProvided
        }
        (TaskRunStatus::AgentRunning, TaskRunCommand::RequireDecision) => {
            TaskRunEvent::DecisionRequired
        }
        (TaskRunStatus::DecisionRequired, TaskRunCommand::ResumeAfterDecision) => {
            TaskRunEvent::DecisionProvided
        }
        (TaskRunStatus::AgentRunning, TaskRunCommand::StartLocalChecks) => {
            TaskRunEvent::LocalChecksStarted
        }
        (
            TaskRunStatus::LocalChecking,
            TaskRunCommand::CompleteLocalChecks {
                required_checks_passed,
                scope_check_passed,
                final_suite_passed,
                behavior_changed,
                red_evidence_present,
                green_evidence_present,
                head_commit,
            },
        ) => {
            if !required_checks_passed || !scope_check_passed || !final_suite_passed {
                return Err(DomainError::LocalChecksFailed);
            }
            if *behavior_changed && (!red_evidence_present || !green_evidence_present) {
                return Err(DomainError::MissingTddEvidence);
            }
            if run
                .health_flags
                .iter()
                .any(|flag| flag.blocks_integration())
            {
                return Err(DomainError::BlockingHealthFlags);
            }
            TaskRunEvent::Succeeded {
                head_commit: head_commit.clone(),
            }
        }
        (
            TaskRunStatus::Queued
            | TaskRunStatus::Preparing
            | TaskRunStatus::AgentRunning
            | TaskRunStatus::InputRequired
            | TaskRunStatus::DecisionRequired
            | TaskRunStatus::LocalChecking,
            TaskRunCommand::Fail { reason },
        ) => TaskRunEvent::Failed {
            reason: reason.clone(),
        },
        (
            TaskRunStatus::Queued
            | TaskRunStatus::Preparing
            | TaskRunStatus::AgentRunning
            | TaskRunStatus::InputRequired
            | TaskRunStatus::DecisionRequired
            | TaskRunStatus::LocalChecking,
            TaskRunCommand::Cancel,
        ) => TaskRunEvent::Cancelled,
        _ => {
            return Err(DomainError::InvalidTransition {
                aggregate: "TaskRun",
                from: run.status.name(),
                command: command.name(),
            });
        }
    };
    Ok(vec![event])
}

#[must_use]
pub fn evolve_task_run(mut run: TaskRun, event: &TaskRunEvent) -> TaskRun {
    match event {
        TaskRunEvent::PreparationStarted => run.status = TaskRunStatus::Preparing,
        TaskRunEvent::PreparationDeferred => run.status = TaskRunStatus::Queued,
        TaskRunEvent::AgentStarted
        | TaskRunEvent::InputProvided
        | TaskRunEvent::DecisionProvided => run.status = TaskRunStatus::AgentRunning,
        TaskRunEvent::InputRequired => run.status = TaskRunStatus::InputRequired,
        TaskRunEvent::DecisionRequired => run.status = TaskRunStatus::DecisionRequired,
        TaskRunEvent::LocalChecksStarted => run.status = TaskRunStatus::LocalChecking,
        TaskRunEvent::Succeeded { head_commit } => {
            run.status = TaskRunStatus::Succeeded;
            run.head_commit = Some(head_commit.clone());
        }
        TaskRunEvent::Failed { .. } => run.status = TaskRunStatus::Failed,
        TaskRunEvent::Cancelled => run.status = TaskRunStatus::Cancelled,
    }
    run
}
