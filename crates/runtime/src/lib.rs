//! Project coordinator runtime and operating-system resource ownership.
#![allow(clippy::missing_errors_doc)]

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
        let fields = source
            .lines()
            .filter_map(|line| line.split_once('='))
            .collect::<std::collections::BTreeMap<_, _>>();
        Some(Self {
            instance_id: fields.get("instance_id")?.to_string(),
            process_id: fields.get("process_id")?.parse().ok()?,
            protocol_version: fields.get("protocol_version")?.parse().ok()?,
            project_root: PathBuf::from(fields.get("project_root")?),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoordinatorError {
    InvalidInstanceId,
    ProjectRootUnavailable(String),
    Io(String),
    AlreadyOwned { owner: Option<CoordinatorMetadata> },
}

impl fmt::Display for CoordinatorError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInstanceId => formatter.write_str("coordinator instance ID is empty"),
            Self::ProjectRootUnavailable(path) => {
                write!(formatter, "project root is unavailable: {path}")
            }
            Self::Io(error) => write!(formatter, "coordinator I/O failed: {error}"),
            Self::AlreadyOwned { owner: Some(owner) } => write!(
                formatter,
                "project writer is already owned by {} (pid {})",
                owner.instance_id, owner.process_id
            ),
            Self::AlreadyOwned { owner: None } => {
                formatter.write_str("project writer is already owned")
            }
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
}

impl ProjectWriterLease {
    pub fn acquire(
        project_root: impl AsRef<Path>,
        instance_id: impl Into<String>,
        protocol_version: u32,
    ) -> Result<Self, CoordinatorError> {
        let instance_id = instance_id.into();
        let instance_id = instance_id.trim();
        if instance_id.is_empty() {
            return Err(CoordinatorError::InvalidInstanceId);
        }
        let requested_root = project_root.as_ref();
        let project_root = requested_root.canonicalize().map_err(|_| {
            CoordinatorError::ProjectRootUnavailable(requested_root.display().to_string())
        })?;
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
                let owner = fs::read_to_string(&metadata_path)
                    .ok()
                    .and_then(|source| CoordinatorMetadata::decode(&source));
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
            let _ = lock_file.unlock();
            return Err(CoordinatorError::from(error));
        }

        Ok(Self {
            lock_file,
            metadata,
        })
    }

    #[must_use]
    pub const fn metadata(&self) -> &CoordinatorMetadata {
        &self.metadata
    }
}

impl Drop for ProjectWriterLease {
    fn drop(&mut self) {
        let _ = self.lock_file.unlock();
    }
}
