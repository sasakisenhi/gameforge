use std::fmt;

use crate::{CommitSha, TaskId};

/// ドメイン境界で拒否された操作。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomainError {
    EmptyValue(&'static str),
    InvalidValue {
        field: &'static str,
        reason: &'static str,
    },
    InvalidCommitSha(String),
    ZeroRevision(&'static str),
    InvalidTransition {
        aggregate: &'static str,
        from: &'static str,
        command: &'static str,
    },
    MissingDependency {
        task_id: TaskId,
        depends_on: TaskId,
    },
    SelfDependency(TaskId),
    DependencyCycle(Vec<TaskId>),
    DuplicateTask(TaskId),
    EmptyDependencyReason,
    MissingLease,
    MissingTddEvidence,
    LocalChecksFailed,
    BlockingHealthFlags,
    TaskRunNotSucceeded,
    CandidateHasNoTaskRuns,
    DuplicateTaskForCandidate(TaskId),
    CandidateNotTechnicallyVerified,
    CommitMismatch {
        expected: CommitSha,
        actual: CommitSha,
    },
    BuildNotReady,
    AcceptanceAlreadyRecorded,
    AcceptanceDoesNotMatch,
    MergeGateRejected(String),
}

impl fmt::Display for DomainError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyValue(field) => write!(formatter, "{field} must not be empty"),
            Self::InvalidValue { field, reason } => {
                write!(formatter, "{field} is invalid: {reason}")
            }
            Self::InvalidCommitSha(value) => write!(formatter, "invalid commit SHA: {value}"),
            Self::ZeroRevision(kind) => write!(formatter, "{kind} must be greater than zero"),
            Self::InvalidTransition {
                aggregate,
                from,
                command,
            } => write!(
                formatter,
                "invalid {aggregate} transition: {command} from {from}"
            ),
            Self::MissingDependency {
                task_id,
                depends_on,
            } => write!(formatter, "{task_id} depends on missing task {depends_on}"),
            Self::SelfDependency(task_id) => write!(formatter, "{task_id} depends on itself"),
            Self::DependencyCycle(tasks) => write!(formatter, "dependency cycle: {tasks:?}"),
            Self::DuplicateTask(task_id) => write!(formatter, "duplicate task: {task_id}"),
            Self::EmptyDependencyReason => write!(formatter, "dependency reason must not be empty"),
            Self::MissingLease => write!(formatter, "resource and worktree leases are required"),
            Self::MissingTddEvidence => write!(formatter, "Red and Green evidence are required"),
            Self::LocalChecksFailed => write!(formatter, "one or more local checks failed"),
            Self::BlockingHealthFlags => write!(formatter, "blocking health flags are present"),
            Self::TaskRunNotSucceeded => write!(formatter, "task run has not succeeded"),
            Self::CandidateHasNoTaskRuns => write!(formatter, "candidate has no task runs"),
            Self::DuplicateTaskForCandidate(task_id) => {
                write!(formatter, "candidate contains multiple runs for {task_id}")
            }
            Self::CandidateNotTechnicallyVerified => {
                write!(formatter, "candidate is not technically verified")
            }
            Self::CommitMismatch { expected, actual } => {
                write!(
                    formatter,
                    "commit mismatch: expected {expected}, got {actual}"
                )
            }
            Self::BuildNotReady => write!(formatter, "build is not ready"),
            Self::AcceptanceAlreadyRecorded => {
                write!(formatter, "acceptance has already been recorded")
            }
            Self::AcceptanceDoesNotMatch => {
                write!(formatter, "acceptance does not match candidate or build")
            }
            Self::MergeGateRejected(reason) => write!(formatter, "merge rejected: {reason}"),
        }
    }
}

impl std::error::Error for DomainError {}
