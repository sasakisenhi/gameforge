use crate::{Candidate, CandidateId, CandidateRevision, CommitSha, DomainError, EvidenceAttemptId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceKind {
    LocalCheck,
    ContinuousIntegration,
    AiReview,
    ConflictAnalysis,
    Build,
    Acceptance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceKey {
    candidate_id: CandidateId,
    candidate_revision: CandidateRevision,
    subject_commit: CommitSha,
    kind: EvidenceKind,
    attempt_id: EvidenceAttemptId,
}

impl EvidenceKey {
    pub fn new(
        candidate_id: CandidateId,
        candidate_revision: CandidateRevision,
        subject_commit: CommitSha,
        kind: EvidenceKind,
        attempt_id: impl Into<String>,
    ) -> Result<Self, DomainError> {
        Ok(Self {
            candidate_id,
            candidate_revision,
            subject_commit,
            kind,
            attempt_id: EvidenceAttemptId::new(attempt_id)?,
        })
    }

    #[must_use]
    pub fn is_current_for(&self, candidate: &Candidate) -> bool {
        self.candidate_id == *candidate.id()
            && self.candidate_revision == candidate.current_revision()
            && self.subject_commit == *candidate.integration_commit()
    }

    #[must_use]
    pub const fn kind(&self) -> EvidenceKind {
        self.kind
    }

    #[must_use]
    pub fn attempt_id(&self) -> &EvidenceAttemptId {
        &self.attempt_id
    }
}
