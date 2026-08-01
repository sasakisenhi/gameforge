use std::{fs, path::Path};

use gameforge_event_journal::EventJournal;
use gameforge_persistence::ProjectionStore;
use gameforge_project_documents::{TaskDocument, load_task_document, validate_task_documents};
use gameforge_runtime::ProjectWriterLease;

use crate::{BootstrapError, ProjectSession, ProjectSnapshot, ProjectValidation};

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
        inbox: projection
            .inbox_rows()
            .map_err(|error| BootstrapError::Projection(error.to_string()))?,
    };

    Ok(ProjectSession {
        snapshot,
        documents,
        journal,
        projection,
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
