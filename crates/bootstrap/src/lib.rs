//! Shared composition root for desktop and headless coordinators.
#![allow(clippy::missing_errors_doc)]

use std::{fmt, fs, path::Path};

use gameforgo_event_journal::EventJournal;
use gameforgo_persistence::{DevelopmentBoardRow, ProjectionStore};
use gameforgo_project_documents::{TaskDocument, load_task_document, validate_task_documents};
use gameforgo_runtime::ProjectWriterLease;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectValidation {
    pub task_count: usize,
    pub integration_order: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectSnapshot {
    pub projection_revision: u64,
    pub development_board: Vec<DevelopmentBoardRow>,
}

pub struct ProjectSession {
    snapshot: ProjectSnapshot,
    _writer_lease: ProjectWriterLease,
}

impl ProjectSession {
    #[must_use]
    pub const fn snapshot(&self) -> &ProjectSnapshot {
        &self.snapshot
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootstrapError {
    Io(String),
    NoTaskDocuments,
    Document(String),
    Journal(String),
    Projection(String),
    Coordinator(String),
}

impl fmt::Display for BootstrapError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "project I/O failed: {error}"),
            Self::NoTaskDocuments => formatter.write_str("no Task documents were found"),
            Self::Document(error) => write!(formatter, "Task document validation failed: {error}"),
            Self::Journal(error) => write!(formatter, "Event Journal failed: {error}"),
            Self::Projection(error) => write!(formatter, "Read Model failed: {error}"),
            Self::Coordinator(error) => write!(formatter, "ProjectCoordinator failed: {error}"),
        }
    }
}

impl std::error::Error for BootstrapError {}

impl From<std::io::Error> for BootstrapError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

pub fn validate_project(
    project_root: impl AsRef<Path>,
) -> Result<ProjectValidation, BootstrapError> {
    let documents = load_task_documents(project_root.as_ref())?;
    let order = validate_task_documents(&documents)
        .map_err(|error| BootstrapError::Document(error.to_string()))?;
    Ok(ProjectValidation {
        task_count: documents.len(),
        integration_order: order
            .into_iter()
            .map(|task_id| task_id.as_str().to_owned())
            .collect(),
    })
}

pub fn rebuild_project(
    project_root: impl AsRef<Path>,
    coordinator_instance_id: &str,
) -> Result<ProjectSnapshot, BootstrapError> {
    let session = start_project(project_root, coordinator_instance_id)?;
    Ok(session.snapshot().clone())
}

pub fn start_project(
    project_root: impl AsRef<Path>,
    coordinator_instance_id: &str,
) -> Result<ProjectSession, BootstrapError> {
    let project_root = project_root.as_ref();
    let writer_lease = ProjectWriterLease::acquire(project_root, coordinator_instance_id, 1)
        .map_err(|error| BootstrapError::Coordinator(error.to_string()))?;
    let documents = load_task_documents(project_root)?;
    validate_task_documents(&documents)
        .map_err(|error| BootstrapError::Document(error.to_string()))?;

    let game_dev = project_root.join(".game-dev");
    let events_directory = game_dev.join("events");
    fs::create_dir_all(&events_directory).map_err(BootstrapError::from)?;
    let journal = EventJournal::open(events_directory.join("events.jsonl"))
        .map_err(|error| BootstrapError::Journal(error.to_string()))?;
    let mut projection = ProjectionStore::open(game_dev.join("read-model.sqlite"))
        .map_err(|error| BootstrapError::Projection(error.to_string()))?;
    projection
        .rebuild(&documents, journal.events())
        .map_err(|error| BootstrapError::Projection(error.to_string()))?;

    let snapshot = ProjectSnapshot {
        projection_revision: projection
            .projection_revision()
            .map_err(|error| BootstrapError::Projection(error.to_string()))?,
        development_board: projection
            .development_board_rows()
            .map_err(|error| BootstrapError::Projection(error.to_string()))?,
    };

    Ok(ProjectSession {
        snapshot,
        _writer_lease: writer_lease,
    })
}

fn load_task_documents(project_root: &Path) -> Result<Vec<TaskDocument>, BootstrapError> {
    let tasks_directory = project_root.join(".game-dev/tasks");
    let entries = fs::read_dir(&tasks_directory).map_err(BootstrapError::from)?;
    let mut paths = entries
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(BootstrapError::from)?;
    paths.retain(|path| path.extension().is_some_and(|extension| extension == "md"));
    paths.sort();
    if paths.is_empty() {
        return Err(BootstrapError::NoTaskDocuments);
    }

    paths
        .into_iter()
        .map(|path| {
            let source = fs::read_to_string(&path).map_err(BootstrapError::from)?;
            load_task_document(&source)
                .map_err(|error| BootstrapError::Document(format!("{}: {error}", path.display())))
        })
        .collect()
}
