use crate::{
    AcceptanceDecision, AcceptanceResult, Build, BuildStatus, Candidate, CandidateStatus,
    CommitSha, DomainError,
};

pub struct MergeGate<'a> {
    pub candidate: &'a Candidate,
    pub build: &'a Build,
    pub acceptance: &'a AcceptanceResult,
    pub pull_request_head: CommitSha,
    pub required_ci_commit: CommitSha,
    pub required_ci_passed: bool,
    pub ai_review_commit: CommitSha,
    pub ai_review_passed: bool,
    pub main_base_is_compatible: bool,
}

impl MergeGate<'_> {
    pub fn evaluate(&self) -> Result<(), DomainError> {
        let commit = self.candidate.integration_commit();
        let governing_result_matches = self
            .candidate
            .governing_acceptance_result_id()
            .is_some_and(|id| id == self.acceptance.id());

        let checks = [
            (
                self.candidate.status() == CandidateStatus::Accepted,
                "candidate is not accepted",
            ),
            (
                self.build.status() == BuildStatus::Ready,
                "accepted build is not ready",
            ),
            (
                self.acceptance.decision() == AcceptanceDecision::Accepted,
                "acceptance decision is not ACCEPTED",
            ),
            (
                self.acceptance.applies_to(self.candidate, self.build),
                "acceptance does not apply to the current candidate revision",
            ),
            (
                governing_result_matches,
                "candidate does not govern with this acceptance result",
            ),
            (
                self.pull_request_head == *commit,
                "pull request head commit differs",
            ),
            (
                self.required_ci_passed && self.required_ci_commit == *commit,
                "required CI is missing, failed, or stale",
            ),
            (
                self.ai_review_passed && self.ai_review_commit == *commit,
                "AI review is missing, failed, or stale",
            ),
            (
                !self.candidate.has_blocking_health_flags(),
                "candidate has blocking health flags",
            ),
            (
                self.main_base_is_compatible,
                "main base is no longer compatible",
            ),
        ];

        if let Some((_, reason)) = checks.into_iter().find(|(passed, _)| !passed) {
            return Err(DomainError::MergeGateRejected(reason.to_owned()));
        }
        Ok(())
    }
}
