use std::fmt;

use crate::DomainError;

macro_rules! string_value {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, DomainError> {
                let value = value.into();
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    return Err(DomainError::EmptyValue(stringify!($name)));
                }
                Ok(Self(trimmed.to_owned()))
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

string_value!(TaskId);
string_value!(TaskRunId);
string_value!(CandidateId);
string_value!(BuildId);
string_value!(PlaytestSessionId);
string_value!(AcceptanceResultId);
string_value!(ArtifactUri);
string_value!(ArtifactHash);
string_value!(Actor);
string_value!(RecordedAt);
string_value!(EvidenceAttemptId);

/// Git object formatに応じたSHA-1またはSHA-256のcommit ID。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CommitSha(String);

impl CommitSha {
    pub fn new(value: impl Into<String>) -> Result<Self, DomainError> {
        let value = value.into();
        if !matches!(value.len(), 40 | 64) || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(DomainError::InvalidCommitSha(value));
        }
        Ok(Self(value.to_ascii_lowercase()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CommitSha {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

macro_rules! revision_value {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(u64);

        impl $name {
            pub fn new(value: u64) -> Result<Self, DomainError> {
                if value == 0 {
                    return Err(DomainError::ZeroRevision(stringify!($name)));
                }
                Ok(Self(value))
            }

            #[must_use]
            pub const fn get(self) -> u64 {
                self.0
            }
        }
    };
}

revision_value!(ContractRevision);
revision_value!(CandidateRevision);
