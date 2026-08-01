use crate::{
    AcceptanceResultId, Actor, Build, BuildId, BuildStatus, Candidate, CandidateRevision,
    CommitSha, DomainError, PlaytestSessionId, RecordedAt,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcceptanceDecision {
    Accepted,
    ChangesRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlaytestStatus {
    Pending,
    Playtesting,
    Completed,
    Aborted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaytestSession {
    id: PlaytestSessionId,
    build_id: BuildId,
    played_commit: CommitSha,
    status: PlaytestStatus,
    acceptance_result_id: Option<AcceptanceResultId>,
}

impl PlaytestSession {
    pub fn start(id: PlaytestSessionId, build: &Build) -> Result<Self, DomainError> {
        if build.status() != BuildStatus::Ready {
            return Err(DomainError::BuildNotReady);
        }
        Ok(Self {
            id,
            build_id: build.id().clone(),
            played_commit: build.source_commit().clone(),
            status: PlaytestStatus::Pending,
            acceptance_result_id: None,
        })
    }

    pub fn begin(&mut self) -> Result<(), DomainError> {
        if self.status != PlaytestStatus::Pending {
            return Err(DomainError::InvalidTransition {
                aggregate: "PlaytestSession",
                from: self.status_name(),
                command: "BeginPlaytest",
            });
        }
        self.status = PlaytestStatus::Playtesting;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_result(
        &mut self,
        build: &Build,
        result_id: AcceptanceResultId,
        decision: AcceptanceDecision,
        reason: impl Into<String>,
        actor: Actor,
        recorded_at: RecordedAt,
    ) -> Result<AcceptanceResult, DomainError> {
        if self.acceptance_result_id.is_some() {
            return Err(DomainError::AcceptanceAlreadyRecorded);
        }
        if self.status != PlaytestStatus::Playtesting {
            return Err(DomainError::InvalidTransition {
                aggregate: "PlaytestSession",
                from: self.status_name(),
                command: "RecordAcceptanceResult",
            });
        }
        if build.status() != BuildStatus::Ready
            || build.id() != &self.build_id
            || build.source_commit() != &self.played_commit
        {
            return Err(DomainError::AcceptanceDoesNotMatch);
        }
        let reason = reason.into();
        if reason.trim().is_empty() {
            return Err(DomainError::EmptyValue("acceptance reason"));
        }
        let result = AcceptanceResult {
            id: result_id.clone(),
            session_id: self.id.clone(),
            build_id: self.build_id.clone(),
            candidate_revision: build.candidate_revision(),
            played_commit: self.played_commit.clone(),
            decision,
            reason: reason.trim().to_owned(),
            actor,
            recorded_at,
        };
        self.acceptance_result_id = Some(result_id);
        self.status = PlaytestStatus::Completed;
        Ok(result)
    }

    pub fn abort(&mut self) -> Result<(), DomainError> {
        if !matches!(
            self.status,
            PlaytestStatus::Pending | PlaytestStatus::Playtesting
        ) {
            return Err(DomainError::InvalidTransition {
                aggregate: "PlaytestSession",
                from: self.status_name(),
                command: "AbortPlaytest",
            });
        }
        self.status = PlaytestStatus::Aborted;
        Ok(())
    }

    const fn status_name(&self) -> &'static str {
        match self.status {
            PlaytestStatus::Pending => "PENDING",
            PlaytestStatus::Playtesting => "PLAYTESTING",
            PlaytestStatus::Completed => "COMPLETED",
            PlaytestStatus::Aborted => "ABORTED",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptanceResult {
    id: AcceptanceResultId,
    session_id: PlaytestSessionId,
    build_id: BuildId,
    candidate_revision: CandidateRevision,
    played_commit: CommitSha,
    decision: AcceptanceDecision,
    reason: String,
    actor: Actor,
    recorded_at: RecordedAt,
}

impl AcceptanceResult {
    #[must_use]
    pub fn id(&self) -> &AcceptanceResultId {
        &self.id
    }

    #[must_use]
    pub fn session_id(&self) -> &PlaytestSessionId {
        &self.session_id
    }

    #[must_use]
    pub fn build_id(&self) -> &BuildId {
        &self.build_id
    }

    #[must_use]
    pub const fn candidate_revision(&self) -> CandidateRevision {
        self.candidate_revision
    }

    #[must_use]
    pub fn played_commit(&self) -> &CommitSha {
        &self.played_commit
    }

    #[must_use]
    pub const fn decision(&self) -> AcceptanceDecision {
        self.decision
    }

    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }

    #[must_use]
    pub fn actor(&self) -> &Actor {
        &self.actor
    }

    #[must_use]
    pub fn recorded_at(&self) -> &RecordedAt {
        &self.recorded_at
    }

    #[must_use]
    pub fn applies_to(&self, candidate: &Candidate, build: &Build) -> bool {
        build.status() == BuildStatus::Ready
            && build.id() == &self.build_id
            && build.candidate_id() == candidate.id()
            && self.candidate_revision == candidate.current_revision()
            && build.candidate_revision() == candidate.current_revision()
            && self.played_commit == *candidate.integration_commit()
            && self.played_commit == *build.source_commit()
    }
}
