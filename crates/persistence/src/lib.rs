//! Rebuildable `SQLite` Read Model.
#![allow(clippy::missing_errors_doc)]

use std::fmt;

pub use gameforge_application::DevelopmentBoardRecord as DevelopmentBoardRow;
use gameforge_event_journal::EventEnvelope;
use gameforge_project_documents::TaskDocument;
use rusqlite::{Connection, OptionalExtension, Transaction, params};

const SCHEMA: &str = r"
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS projection_meta (
    key TEXT PRIMARY KEY,
    value INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS applied_events (
    event_id TEXT PRIMARY KEY,
    aggregate_type TEXT NOT NULL,
    aggregate_id TEXT NOT NULL,
    aggregate_version INTEGER NOT NULL,
    event_type TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS development_board_rows (
    task_id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    task_status TEXT NOT NULL,
    current_run_id TEXT,
    run_status TEXT
);

INSERT OR IGNORE INTO projection_meta(key, value) VALUES ('revision', 0);
";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectionError {
    Database(String),
    MissingPayloadField {
        event_type: String,
        field: &'static str,
    },
    TaskNotFound {
        event_type: String,
        task_id: String,
    },
    TaskRunIdMismatch {
        event_type: String,
        payload_run_id: String,
        aggregate_id: String,
    },
    AggregateVersion {
        aggregate_type: String,
        aggregate_id: String,
        expected: u64,
        actual: u64,
    },
    IntegerOutOfRange {
        field: &'static str,
        value: u64,
    },
}

impl fmt::Display for ProjectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Database(error) => write!(formatter, "projection database failed: {error}"),
            Self::MissingPayloadField { event_type, field } => {
                write!(formatter, "{event_type} is missing payload field {field}")
            }
            Self::TaskNotFound {
                event_type,
                task_id,
            } => write!(formatter, "{event_type} references missing Task {task_id}"),
            Self::TaskRunIdMismatch {
                event_type,
                payload_run_id,
                aggregate_id,
            } => write!(
                formatter,
                "{event_type} payload run_id {payload_run_id} does not match TaskRun aggregate ID {aggregate_id}"
            ),
            Self::AggregateVersion {
                aggregate_type,
                aggregate_id,
                expected,
                actual,
            } => write!(
                formatter,
                "aggregate version mismatch for {aggregate_type}/{aggregate_id}: expected {expected}, got {actual}"
            ),
            Self::IntegerOutOfRange { field, value } => {
                write!(formatter, "{field} does not fit SQLite INTEGER: {value}")
            }
        }
    }
}

impl std::error::Error for ProjectionError {}

impl From<rusqlite::Error> for ProjectionError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Database(error.to_string())
    }
}

pub struct ProjectionStore {
    connection: Connection,
}

impl ProjectionStore {
    pub fn open_in_memory() -> Result<Self, ProjectionError> {
        let connection = Connection::open_in_memory()?;
        connection.execute_batch(SCHEMA)?;
        Ok(Self { connection })
    }

    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Self, ProjectionError> {
        let connection = Connection::open(path)?;
        connection.execute_batch(SCHEMA)?;
        Ok(Self { connection })
    }

    pub fn rebuild(
        &mut self,
        documents: &[TaskDocument],
        events: &[EventEnvelope],
    ) -> Result<(), ProjectionError> {
        let transaction = self.connection.transaction()?;
        transaction.execute("DELETE FROM applied_events", [])?;
        transaction.execute("DELETE FROM development_board_rows", [])?;
        transaction.execute(
            "UPDATE projection_meta SET value = 0 WHERE key = 'revision'",
            [],
        )?;
        for document in documents {
            transaction.execute(
                "INSERT INTO development_board_rows(
                    task_id, title, task_status, current_run_id, run_status
                 ) VALUES (?1, ?2, ?3, NULL, NULL)",
                params![
                    document.id().as_str(),
                    document.title(),
                    document.status().as_str()
                ],
            )?;
        }
        for event in events {
            apply_event_in_transaction(&transaction, event)?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn apply_event(&mut self, event: &EventEnvelope) -> Result<(), ProjectionError> {
        let transaction = self.connection.transaction()?;
        apply_event_in_transaction(&transaction, event)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn projection_revision(&self) -> Result<u64, ProjectionError> {
        let revision = self.connection.query_row(
            "SELECT value FROM projection_meta WHERE key = 'revision'",
            [],
            |row| row.get::<_, i64>(0),
        )?;
        u64::try_from(revision).map_err(|_| {
            ProjectionError::Database(format!("negative projection revision: {revision}"))
        })
    }

    pub fn development_board_rows(&self) -> Result<Vec<DevelopmentBoardRow>, ProjectionError> {
        let mut statement = self.connection.prepare(
            "SELECT task_id, title, task_status, current_run_id, run_status
             FROM development_board_rows ORDER BY task_id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(DevelopmentBoardRow {
                task_id: row.get(0)?,
                title: row.get(1)?,
                task_status: row.get(2)?,
                current_run_id: row.get(3)?,
                run_status: row.get(4)?,
                health_flags: Vec::new(),
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}

fn apply_event_in_transaction(
    transaction: &Transaction<'_>,
    event: &EventEnvelope,
) -> Result<(), ProjectionError> {
    let header = event.header();
    let already_applied = transaction
        .query_row(
            "SELECT 1 FROM applied_events WHERE event_id = ?1",
            [header.event_id.as_str()],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if already_applied {
        return Ok(());
    }

    let last_version = transaction
        .query_row(
            "SELECT MAX(aggregate_version) FROM applied_events
             WHERE aggregate_type = ?1 AND aggregate_id = ?2",
            params![
                header.aggregate.aggregate_type(),
                header.aggregate.aggregate_id()
            ],
            |row| row.get::<_, Option<i64>>(0),
        )?
        .unwrap_or(0);
    let last_version = u64::try_from(last_version).map_err(|_| {
        ProjectionError::Database(format!("negative aggregate version: {last_version}"))
    })?;
    let expected = last_version + 1;
    if header.aggregate_version != expected {
        return Err(ProjectionError::AggregateVersion {
            aggregate_type: header.aggregate.aggregate_type().to_owned(),
            aggregate_id: header.aggregate.aggregate_id().to_owned(),
            expected,
            actual: header.aggregate_version,
        });
    }

    match event.event_type() {
        "TaskRunQueued" | "TaskRunStateChanged" => {
            let task_id = payload(event, "task_id")?;
            let run_id = payload(event, "run_id")?;
            let aggregate_id = header.aggregate.aggregate_id();
            if run_id != aggregate_id {
                return Err(ProjectionError::TaskRunIdMismatch {
                    event_type: event.event_type().to_owned(),
                    payload_run_id: run_id.to_owned(),
                    aggregate_id: aggregate_id.to_owned(),
                });
            }
            let state = event
                .payload()
                .get("state")
                .map_or("QUEUED", String::as_str);
            let current_run_id = (state != "CANCELLED").then_some(run_id);
            let updated = transaction.execute(
                "UPDATE development_board_rows
                 SET current_run_id = ?1, run_status = ?2
                 WHERE task_id = ?3",
                params![current_run_id, state, task_id],
            )?;
            if updated == 0 {
                return Err(ProjectionError::TaskNotFound {
                    event_type: event.event_type().to_owned(),
                    task_id: task_id.to_owned(),
                });
            }
        }
        _ => {}
    }

    let aggregate_version = i64::try_from(header.aggregate_version).map_err(|_| {
        ProjectionError::IntegerOutOfRange {
            field: "aggregate_version",
            value: header.aggregate_version,
        }
    })?;
    transaction.execute(
        "INSERT INTO applied_events(
            event_id, aggregate_type, aggregate_id, aggregate_version, event_type
         ) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            header.event_id,
            header.aggregate.aggregate_type(),
            header.aggregate.aggregate_id(),
            aggregate_version,
            event.event_type()
        ],
    )?;
    transaction.execute(
        "UPDATE projection_meta SET value = value + 1 WHERE key = 'revision'",
        [],
    )?;
    Ok(())
}

fn payload<'a>(event: &'a EventEnvelope, field: &'static str) -> Result<&'a str, ProjectionError> {
    event
        .payload()
        .get(field)
        .map(String::as_str)
        .ok_or_else(|| ProjectionError::MissingPayloadField {
            event_type: event.event_type().to_owned(),
            field,
        })
}
