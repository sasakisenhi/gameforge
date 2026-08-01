//! Project coordinator runtime and operating-system resource ownership.
#![allow(clippy::missing_errors_doc)]

mod scheduler;

pub use scheduler::{
    DEFAULT_MAX_CONCURRENT_TASK_RUNS, DeferralReason, DeferredRun, QueuedRun, ScheduleConfig,
    SchedulePlan, ScheduleSnapshot, StartBlocker, plan_schedule,
};

use std::{
    fmt, fs,
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoordinatorMetadata {
    pub instance_id: String,
    pub process_id: u32,
    pub protocol_version: u32,
    pub project_root: PathBuf,
}

impl CoordinatorMetadata {
    fn encode(&self) -> String {
        format!(
            "instance_id={}\nprocess_id={}\nprotocol_version={}\nproject_root={}\n",
            self.instance_id,
            self.process_id,
            self.protocol_version,
            self.project_root.display()
        )
    }

    fn decode(source: &str) -> Option<Self> {
        let mut instance_id = None;
        let mut process_id = None;
        let mut protocol_version = None;
        let mut project_root = None;
        for line in source.lines() {
            let (key, value) = line.split_once('=')?;
            let destination = match key {
                "instance_id" if instance_id.is_none() => &mut instance_id,
                "process_id" if process_id.is_none() => &mut process_id,
                "protocol_version" if protocol_version.is_none() => &mut protocol_version,
                "project_root" if project_root.is_none() => &mut project_root,
                _ => return None,
            };
            *destination = Some(value);
        }

        let instance_id = instance_id?;
        let project_root = PathBuf::from(project_root?);
        if instance_id.trim().is_empty() || !project_root.is_absolute() {
            return None;
        }
        Some(Self {
            instance_id: instance_id.to_owned(),
            process_id: process_id?.parse().ok()?,
            protocol_version: protocol_version?.parse().ok()?,
            project_root,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoordinatorError {
    InvalidInstanceId,
    ProjectRootUnavailable(String),
    Io(String),
    MetadataMissing { path: PathBuf },
    MetadataInvalid { path: PathBuf },
    AlreadyOwned { owner: CoordinatorMetadata },
}

impl fmt::Display for CoordinatorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInstanceId => formatter.write_str("coordinator instance ID is empty"),
            Self::ProjectRootUnavailable(path) => {
                write!(formatter, "project root is unavailable: {path}")
            }
            Self::Io(error) => write!(formatter, "coordinator I/O failed: {error}"),
            Self::MetadataMissing { path } => write!(
                formatter,
                "coordinator holds the writer lock but metadata is missing: {}",
                path.display()
            ),
            Self::MetadataInvalid { path } => write!(
                formatter,
                "coordinator holds the writer lock but metadata is invalid: {}",
                path.display()
            ),
            Self::AlreadyOwned { owner } => write!(
                formatter,
                "project writer is already owned by {} (pid {})",
                owner.instance_id, owner.process_id
            ),
        }
    }
}

impl std::error::Error for CoordinatorError {}

impl From<std::io::Error> for CoordinatorError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

pub struct ProjectWriterLease {
    lock_file: File,
    metadata: CoordinatorMetadata,
    metadata_path: PathBuf,
}

impl ProjectWriterLease {
    pub fn acquire(
        project_root: impl AsRef<Path>,
        instance_id: impl Into<String>,
        protocol_version: u32,
    ) -> Result<Self, CoordinatorError> {
        let instance_id = instance_id.into();
        let instance_id = instance_id.trim();
        if instance_id.is_empty() || instance_id.contains(['\n', '\r']) {
            return Err(CoordinatorError::InvalidInstanceId);
        }
        let requested_root = project_root.as_ref();
        let project_root = canonical_project_root(requested_root)?;
        let runtime_directory = project_root.join(".game-dev/runtime");
        fs::create_dir_all(&runtime_directory).map_err(CoordinatorError::from)?;
        let lock_path = runtime_directory.join("writer.lock");
        let metadata_path = runtime_directory.join("coordinator.meta");
        let lock_file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(CoordinatorError::from)?;

        if let Err(error) = lock_file.try_lock() {
            if matches!(error, std::fs::TryLockError::WouldBlock) {
                let owner = read_locked_metadata(&metadata_path, &project_root)?;
                return Err(CoordinatorError::AlreadyOwned { owner });
            }
            let std::fs::TryLockError::Error(error) = error else {
                unreachable!("WouldBlock was handled above")
            };
            return Err(CoordinatorError::from(error));
        }

        let metadata = CoordinatorMetadata {
            instance_id: instance_id.to_owned(),
            process_id: std::process::id(),
            protocol_version,
            project_root,
        };
        let temporary_metadata =
            runtime_directory.join(format!("coordinator.meta.{}.tmp", metadata.process_id));
        if let Err(error) = fs::write(&temporary_metadata, metadata.encode())
            .and_then(|()| fs::rename(&temporary_metadata, &metadata_path))
        {
            let _ = fs::remove_file(&temporary_metadata);
            let _ = lock_file.unlock();
            return Err(CoordinatorError::from(error));
        }

        Ok(Self {
            lock_file,
            metadata,
            metadata_path,
        })
    }

    #[must_use]
    pub const fn metadata(&self) -> &CoordinatorMetadata {
        &self.metadata
    }
}

impl Drop for ProjectWriterLease {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.metadata_path);
        let _ = self.lock_file.unlock();
    }
}

/// Reports the coordinator currently holding the project's writer lock.
///
/// Metadata is only returned when the lock is held. A metadata file left behind
/// by a crashed process therefore does not make the coordinator appear active.
pub fn coordinator_status(
    project_root: impl AsRef<Path>,
) -> Result<Option<CoordinatorMetadata>, CoordinatorError> {
    let project_root = canonical_project_root(project_root.as_ref())?;
    let runtime_directory = project_root.join(".game-dev/runtime");
    let lock_path = runtime_directory.join("writer.lock");
    let metadata_path = runtime_directory.join("coordinator.meta");
    let lock_file = match OpenOptions::new().read(true).write(true).open(lock_path) {
        Ok(lock_file) => lock_file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(CoordinatorError::from(error)),
    };

    match lock_file.try_lock() {
        Ok(()) => {
            lock_file.unlock().map_err(CoordinatorError::from)?;
            Ok(None)
        }
        Err(std::fs::TryLockError::WouldBlock) => {
            read_locked_metadata(&metadata_path, &project_root).map(Some)
        }
        Err(std::fs::TryLockError::Error(error)) => Err(CoordinatorError::from(error)),
    }
}

fn canonical_project_root(project_root: &Path) -> Result<PathBuf, CoordinatorError> {
    project_root
        .canonicalize()
        .map_err(|_| CoordinatorError::ProjectRootUnavailable(project_root.display().to_string()))
}

fn read_locked_metadata(
    metadata_path: &Path,
    project_root: &Path,
) -> Result<CoordinatorMetadata, CoordinatorError> {
    let source = match fs::read_to_string(metadata_path) {
        Ok(source) => source,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(CoordinatorError::MetadataMissing {
                path: metadata_path.to_owned(),
            });
        }
        Err(error) => return Err(CoordinatorError::from(error)),
    };
    let metadata =
        CoordinatorMetadata::decode(&source).ok_or_else(|| CoordinatorError::MetadataInvalid {
            path: metadata_path.to_owned(),
        })?;
    if metadata.project_root != project_root {
        return Err(CoordinatorError::MetadataInvalid {
            path: metadata_path.to_owned(),
        });
    }
    Ok(metadata)
}
