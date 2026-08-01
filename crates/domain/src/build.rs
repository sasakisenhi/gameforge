use crate::{
    ArtifactHash, ArtifactUri, BuildId, Candidate, CandidateId, CandidateRevision, CandidateStatus,
    CommitSha, DomainError, TaskRunId,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    uri: ArtifactUri,
    hash: ArtifactHash,
}

impl Artifact {
    #[must_use]
    pub fn uri(&self) -> &ArtifactUri {
        &self.uri
    }

    #[must_use]
    pub fn hash(&self) -> &ArtifactHash {
        &self.hash
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildStatus {
    Requested,
    Building,
    Ready,
    Failed,
    Superseded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Build {
    id: BuildId,
    candidate_id: CandidateId,
    candidate_revision: CandidateRevision,
    source_commit: CommitSha,
    included_task_runs: Vec<TaskRunId>,
    status: BuildStatus,
    artifact: Option<Artifact>,
}

impl Build {
    pub fn request(id: BuildId, candidate: &Candidate) -> Result<Self, DomainError> {
        if candidate.status() != CandidateStatus::TechnicallyVerified {
            return Err(DomainError::CandidateNotTechnicallyVerified);
        }
        Ok(Self {
            id,
            candidate_id: candidate.id().clone(),
            candidate_revision: candidate.current_revision(),
            source_commit: candidate.integration_commit().clone(),
            included_task_runs: candidate
                .included_task_runs()
                .iter()
                .map(|included| included.task_run_id().clone())
                .collect(),
            status: BuildStatus::Requested,
            artifact: None,
        })
    }

    #[must_use]
    pub fn id(&self) -> &BuildId {
        &self.id
    }

    #[must_use]
    pub fn candidate_id(&self) -> &CandidateId {
        &self.candidate_id
    }

    #[must_use]
    pub const fn candidate_revision(&self) -> CandidateRevision {
        self.candidate_revision
    }

    #[must_use]
    pub fn source_commit(&self) -> &CommitSha {
        &self.source_commit
    }

    #[must_use]
    pub fn included_task_runs(&self) -> &[TaskRunId] {
        &self.included_task_runs
    }

    #[must_use]
    pub const fn status(&self) -> BuildStatus {
        self.status
    }

    #[must_use]
    pub fn artifact(&self) -> Option<&Artifact> {
        self.artifact.as_ref()
    }

    pub fn start(&mut self) -> Result<(), DomainError> {
        if self.status != BuildStatus::Requested {
            return Err(DomainError::InvalidTransition {
                aggregate: "Build",
                from: self.status_name(),
                command: "StartBuild",
            });
        }
        self.status = BuildStatus::Building;
        Ok(())
    }

    pub fn complete(
        &mut self,
        actual_input_commit: CommitSha,
        artifact_uri: ArtifactUri,
        artifact_hash: ArtifactHash,
    ) -> Result<(), DomainError> {
        if self.status != BuildStatus::Building {
            return Err(DomainError::InvalidTransition {
                aggregate: "Build",
                from: self.status_name(),
                command: "CompleteBuild",
            });
        }
        if actual_input_commit != self.source_commit {
            return Err(DomainError::CommitMismatch {
                expected: self.source_commit.clone(),
                actual: actual_input_commit,
            });
        }
        self.artifact = Some(Artifact {
            uri: artifact_uri,
            hash: artifact_hash,
        });
        self.status = BuildStatus::Ready;
        Ok(())
    }

    pub fn fail(&mut self) -> Result<(), DomainError> {
        if self.status != BuildStatus::Building {
            return Err(DomainError::InvalidTransition {
                aggregate: "Build",
                from: self.status_name(),
                command: "FailBuild",
            });
        }
        self.status = BuildStatus::Failed;
        Ok(())
    }

    pub fn supersede(&mut self) -> Result<(), DomainError> {
        if self.status != BuildStatus::Ready {
            return Err(DomainError::InvalidTransition {
                aggregate: "Build",
                from: self.status_name(),
                command: "SupersedeBuild",
            });
        }
        self.status = BuildStatus::Superseded;
        Ok(())
    }

    const fn status_name(&self) -> &'static str {
        match self.status {
            BuildStatus::Requested => "REQUESTED",
            BuildStatus::Building => "BUILDING",
            BuildStatus::Ready => "READY",
            BuildStatus::Failed => "FAILED",
            BuildStatus::Superseded => "SUPERSEDED",
        }
    }
}
