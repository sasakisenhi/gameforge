use std::collections::BTreeSet;

use crate::{
    AcceptanceDecision, AcceptanceResult, AcceptanceResultId, Build, CandidateId,
    CandidateRevision, CommitSha, DomainError, HealthFlag, TaskId, TaskRun, TaskRunId,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncludedTaskRun {
    task_id: TaskId,
    task_run_id: TaskRunId,
    output_commit: CommitSha,
}

impl IncludedTaskRun {
    pub fn from_succeeded(run: &TaskRun) -> Result<Self, DomainError> {
        if !run.is_integrable() {
            return Err(DomainError::TaskRunNotSucceeded);
        }
        Ok(Self {
            task_id: run.task_id().clone(),
            task_run_id: run.id().clone(),
            output_commit: run
                .head_commit()
                .ok_or(DomainError::TaskRunNotSucceeded)?
                .clone(),
        })
    }

    #[must_use]
    pub fn task_id(&self) -> &TaskId {
        &self.task_id
    }

    #[must_use]
    pub fn task_run_id(&self) -> &TaskRunId {
        &self.task_run_id
    }

    #[must_use]
    pub fn output_commit(&self) -> &CommitSha {
        &self.output_commit
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateStatus {
    TechnicallyVerified,
    Accepted,
    Revising,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    id: CandidateId,
    current_revision: CandidateRevision,
    integration_commit: CommitSha,
    included_task_runs: Vec<IncludedTaskRun>,
    status: CandidateStatus,
    governing_acceptance_result_id: Option<AcceptanceResultId>,
    health_flags: BTreeSet<HealthFlag>,
}

impl Candidate {
    pub fn technically_verified(
        id: CandidateId,
        current_revision: CandidateRevision,
        integration_commit: CommitSha,
        included_task_runs: Vec<IncludedTaskRun>,
    ) -> Result<Self, DomainError> {
        if included_task_runs.is_empty() {
            return Err(DomainError::CandidateHasNoTaskRuns);
        }
        let mut task_ids = BTreeSet::new();
        for included in &included_task_runs {
            if !task_ids.insert(included.task_id.clone()) {
                return Err(DomainError::DuplicateTaskForCandidate(
                    included.task_id.clone(),
                ));
            }
        }
        Ok(Self {
            id,
            current_revision,
            integration_commit,
            included_task_runs,
            status: CandidateStatus::TechnicallyVerified,
            governing_acceptance_result_id: None,
            health_flags: BTreeSet::new(),
        })
    }

    #[must_use]
    pub fn id(&self) -> &CandidateId {
        &self.id
    }

    #[must_use]
    pub const fn current_revision(&self) -> CandidateRevision {
        self.current_revision
    }

    #[must_use]
    pub fn integration_commit(&self) -> &CommitSha {
        &self.integration_commit
    }

    #[must_use]
    pub fn included_task_runs(&self) -> &[IncludedTaskRun] {
        &self.included_task_runs
    }

    #[must_use]
    pub const fn status(&self) -> CandidateStatus {
        self.status
    }

    #[must_use]
    pub fn governing_acceptance_result_id(&self) -> Option<&AcceptanceResultId> {
        self.governing_acceptance_result_id.as_ref()
    }

    #[must_use]
    pub fn has_blocking_health_flags(&self) -> bool {
        !self.health_flags.is_empty()
    }

    #[must_use]
    pub fn with_health_flag(mut self, flag: HealthFlag) -> Self {
        self.health_flags.insert(flag);
        self
    }

    pub fn apply_acceptance(
        &mut self,
        build: &Build,
        result: &AcceptanceResult,
    ) -> Result<(), DomainError> {
        if !result.applies_to(self, build) {
            return Err(DomainError::AcceptanceDoesNotMatch);
        }
        match result.decision() {
            AcceptanceDecision::Accepted => {
                self.status = CandidateStatus::Accepted;
                self.governing_acceptance_result_id = Some(result.id().clone());
            }
            AcceptanceDecision::ChangesRequired => {
                self.status = CandidateStatus::Revising;
                self.governing_acceptance_result_id = None;
            }
        }
        Ok(())
    }
}
