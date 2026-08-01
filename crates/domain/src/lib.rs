//! AIゲーム開発コントロールプレーンの純粋なドメインモデル。
//!
//! このcrateはI/Oやframeworkへ依存せず、状態遷移、不変条件、証拠の
//! commit整合性だけを扱う。
#![allow(
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::needless_pass_by_value
)]

mod acceptance;
mod build;
mod candidate;
mod error;
mod evidence;
mod id;
mod merge_gate;
mod task;
mod task_run;

pub use acceptance::{AcceptanceDecision, AcceptanceResult, PlaytestSession};
pub use build::{Artifact, Build, BuildStatus};
pub use candidate::{Candidate, CandidateStatus, IncludedTaskRun};
pub use error::DomainError;
pub use evidence::{EvidenceKey, EvidenceKind};
pub use id::{
    AcceptanceResultId, Actor, ArtifactHash, ArtifactUri, BuildId, CandidateId, CandidateRevision,
    CommitSha, ContractRevision, EvidenceAttemptId, PlaytestSessionId, RecordedAt, TaskId,
    TaskRunId,
};
pub use merge_gate::MergeGate;
pub use task::{
    Task, TaskCommand, TaskDependency, TaskDependencyKind, TaskEvent, TaskStatus, decide_task,
    evolve_task, validate_task_graph,
};
pub use task_run::{
    HealthFlag, TaskRun, TaskRunCommand, TaskRunEvent, TaskRunStatus, decide_task_run,
    evolve_task_run,
};
