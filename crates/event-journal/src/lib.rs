//! 追記専用Event Journal。
#![allow(clippy::missing_errors_doc)]

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    fs::OpenOptions,
    io::{Read, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

pub const SUPPORTED_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AggregateRef {
    aggregate_type: String,
    aggregate_id: String,
}

impl AggregateRef {
    pub fn new(
        aggregate_type: impl Into<String>,
        aggregate_id: impl Into<String>,
    ) -> Result<Self, JournalError> {
        let aggregate_type = aggregate_type.into();
        let aggregate_type = required("aggregate_type", &aggregate_type)?;
        let aggregate_id = aggregate_id.into();
        let aggregate_id = required("aggregate_id", &aggregate_id)?;
        Ok(Self {
            aggregate_type,
            aggregate_id,
        })
    }

    #[must_use]
    pub fn aggregate_type(&self) -> &str {
        &self.aggregate_type
    }

    #[must_use]
    pub fn aggregate_id(&self) -> &str {
        &self.aggregate_id
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventHeader {
    pub event_id: String,
    pub schema_version: u32,
    pub occurred_at: String,
    pub aggregate: AggregateRef,
    pub aggregate_version: u64,
    pub correlation_id: String,
    pub causation_id: Option<String>,
    pub actor: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventEnvelope {
    header: EventHeader,
    event_type: String,
    payload: BTreeMap<String, String>,
}

impl EventEnvelope {
    pub fn new(
        mut header: EventHeader,
        event_type: impl Into<String>,
        payload: BTreeMap<String, String>,
    ) -> Result<Self, JournalError> {
        header.event_id = required("event_id", &header.event_id)?;
        header.occurred_at = required("occurred_at", &header.occurred_at)?;
        header.correlation_id = required("correlation_id", &header.correlation_id)?;
        header.actor = required("actor", &header.actor)?;
        if header.schema_version != SUPPORTED_SCHEMA_VERSION {
            return Err(JournalError::UnsupportedSchemaVersion(
                header.schema_version,
            ));
        }
        if header.aggregate_version == 0 {
            return Err(JournalError::InvalidField {
                field: "aggregate_version",
                reason: "must be greater than zero".to_owned(),
            });
        }
        let event_type = event_type.into();
        let event_type = required("event_type", &event_type)?;
        Ok(Self {
            header,
            event_type,
            payload,
        })
    }

    #[must_use]
    pub const fn header(&self) -> &EventHeader {
        &self.header
    }

    #[must_use]
    pub fn event_type(&self) -> &str {
        &self.event_type
    }

    #[must_use]
    pub const fn payload(&self) -> &BTreeMap<String, String> {
        &self.payload
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JournalError {
    Io(String),
    InvalidRecord {
        line: usize,
        reason: String,
    },
    InvalidField {
        field: &'static str,
        reason: String,
    },
    UnsupportedSchemaVersion(u32),
    DuplicateEventId(String),
    AggregateVersion {
        aggregate_type: String,
        aggregate_id: String,
        expected: u64,
        actual: u64,
    },
}

impl fmt::Display for JournalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "journal I/O failed: {error}"),
            Self::InvalidRecord { line, reason } => {
                write!(formatter, "invalid journal record at line {line}: {reason}")
            }
            Self::InvalidField { field, reason } => write!(formatter, "{field}: {reason}"),
            Self::UnsupportedSchemaVersion(version) => {
                write!(formatter, "unsupported schema version: {version}")
            }
            Self::DuplicateEventId(id) => write!(formatter, "duplicate event ID: {id}"),
            Self::AggregateVersion {
                aggregate_type,
                aggregate_id,
                expected,
                actual,
            } => write!(
                formatter,
                "aggregate version mismatch for {aggregate_type}/{aggregate_id}: expected {expected}, got {actual}"
            ),
        }
    }
}

impl std::error::Error for JournalError {}

impl From<std::io::Error> for JournalError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

pub struct EventJournal {
    path: PathBuf,
    events: Vec<EventEnvelope>,
    event_ids: BTreeSet<String>,
    aggregate_versions: BTreeMap<(String, String), u64>,
}

impl EventJournal {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, JournalError> {
        let path = path.as_ref().to_path_buf();
        let mut file = OpenOptions::new()
            .read(true)
            .append(true)
            .create(true)
            .open(&path)
            .map_err(JournalError::from)?;
        let mut content = String::new();
        file.read_to_string(&mut content)
            .map_err(JournalError::from)?;

        let mut journal = Self {
            path,
            events: Vec::new(),
            event_ids: BTreeSet::new(),
            aggregate_versions: BTreeMap::new(),
        };
        for (index, line) in content.lines().enumerate() {
            if line.trim().is_empty() {
                return Err(JournalError::InvalidRecord {
                    line: index + 1,
                    reason: "empty records are not allowed".to_owned(),
                });
            }
            let event: EventEnvelope =
                serde_json::from_str(line).map_err(|error| JournalError::InvalidRecord {
                    line: index + 1,
                    reason: error.to_string(),
                })?;
            validate_envelope(&event)?;
            journal.validate_sequence(&event)?;
            journal.record(event);
        }
        Ok(journal)
    }

    pub fn append(&mut self, event: &EventEnvelope) -> Result<(), JournalError> {
        validate_envelope(event)?;
        self.validate_sequence(event)?;
        let mut record =
            serde_json::to_vec(event).map_err(|error| JournalError::InvalidRecord {
                line: self.events.len() + 1,
                reason: error.to_string(),
            })?;
        record.push(b'\n');
        let mut file = OpenOptions::new()
            .append(true)
            .open(&self.path)
            .map_err(JournalError::from)?;
        file.write_all(&record).map_err(JournalError::from)?;
        file.sync_data().map_err(JournalError::from)?;
        self.record(event.clone());
        Ok(())
    }

    #[must_use]
    pub fn events(&self) -> &[EventEnvelope] {
        &self.events
    }

    fn validate_sequence(&self, event: &EventEnvelope) -> Result<(), JournalError> {
        if self.event_ids.contains(&event.header.event_id) {
            return Err(JournalError::DuplicateEventId(
                event.header.event_id.clone(),
            ));
        }
        let key = (
            event.header.aggregate.aggregate_type.clone(),
            event.header.aggregate.aggregate_id.clone(),
        );
        let expected = self.aggregate_versions.get(&key).copied().unwrap_or(0) + 1;
        if event.header.aggregate_version != expected {
            return Err(JournalError::AggregateVersion {
                aggregate_type: key.0,
                aggregate_id: key.1,
                expected,
                actual: event.header.aggregate_version,
            });
        }
        Ok(())
    }

    fn record(&mut self, event: EventEnvelope) {
        self.event_ids.insert(event.header.event_id.clone());
        self.aggregate_versions.insert(
            (
                event.header.aggregate.aggregate_type.clone(),
                event.header.aggregate.aggregate_id.clone(),
            ),
            event.header.aggregate_version,
        );
        self.events.push(event);
    }
}

fn validate_envelope(event: &EventEnvelope) -> Result<(), JournalError> {
    if event.header.schema_version != SUPPORTED_SCHEMA_VERSION {
        return Err(JournalError::UnsupportedSchemaVersion(
            event.header.schema_version,
        ));
    }
    required("event_id", &event.header.event_id)?;
    required("occurred_at", &event.header.occurred_at)?;
    required("correlation_id", &event.header.correlation_id)?;
    required("actor", &event.header.actor)?;
    required("event_type", &event.event_type)?;
    required("aggregate_type", &event.header.aggregate.aggregate_type)?;
    required("aggregate_id", &event.header.aggregate.aggregate_id)?;
    if event.header.aggregate_version == 0 {
        return Err(JournalError::InvalidField {
            field: "aggregate_version",
            reason: "must be greater than zero".to_owned(),
        });
    }
    Ok(())
}

fn required(field: &'static str, value: &str) -> Result<String, JournalError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(JournalError::InvalidField {
            field,
            reason: "must not be empty".to_owned(),
        });
    }
    Ok(trimmed.to_owned())
}
