//! Project Document loader for Git-managed Markdown files.
#![allow(clippy::missing_errors_doc)]

use std::{collections::BTreeSet, fmt};

use gameforge_domain::{
    ContractRevision, DomainError, TaskDependency, TaskDependencyKind, TaskId, validate_task_graph,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};

pub const SUPPORTED_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentError {
    MissingFrontMatter,
    InvalidFrontMatter(String),
    UnsupportedSchemaVersion(u32),
    InvalidDomainValue(String),
    UnsafePath(String),
    EmptyAllowedPaths,
    ConflictingPath(String),
    InvalidTaskGraph(String),
}

impl fmt::Display for DocumentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingFrontMatter => formatter.write_str("YAML front matter is missing"),
            Self::InvalidFrontMatter(error) => write!(formatter, "invalid front matter: {error}"),
            Self::UnsupportedSchemaVersion(version) => {
                write!(formatter, "unsupported schema version: {version}")
            }
            Self::InvalidDomainValue(error) => write!(formatter, "invalid domain value: {error}"),
            Self::UnsafePath(path) => write!(formatter, "unsafe project-relative path: {path}"),
            Self::EmptyAllowedPaths => formatter.write_str("allowed_paths must not be empty"),
            Self::ConflictingPath(path) => {
                write!(formatter, "path is both allowed and forbidden: {path}")
            }
            Self::InvalidTaskGraph(error) => write!(formatter, "invalid task graph: {error}"),
        }
    }
}

impl std::error::Error for DocumentError {}

/// A reason why one changed path violates a task contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangedPathViolation {
    InvalidRelativePath { path: String },
    ForbiddenPath { path: String, pattern: String },
    OutsideAllowedAndTestPaths { path: String },
}

impl ChangedPathViolation {
    #[must_use]
    pub fn path(&self) -> &str {
        match self {
            Self::InvalidRelativePath { path }
            | Self::ForbiddenPath { path, .. }
            | Self::OutsideAllowedAndTestPaths { path } => path,
        }
    }

    const fn sort_order(&self) -> u8 {
        match self {
            Self::InvalidRelativePath { .. } => 0,
            Self::ForbiddenPath { .. } => 1,
            Self::OutsideAllowedAndTestPaths { .. } => 2,
        }
    }
}

impl fmt::Display for ChangedPathViolation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRelativePath { path } => {
                write!(formatter, "invalid project-relative changed path: {path}")
            }
            Self::ForbiddenPath { path, pattern } => {
                write!(
                    formatter,
                    "changed path matches forbidden pattern {pattern}: {path}"
                )
            }
            Self::OutsideAllowedAndTestPaths { path } => {
                write!(
                    formatter,
                    "changed path is outside allowed_paths and test_paths: {path}"
                )
            }
        }
    }
}

/// All changed path violations found in one validation pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedPathValidationError {
    violations: Vec<ChangedPathViolation>,
}

impl ChangedPathValidationError {
    #[must_use]
    pub fn violations(&self) -> &[ChangedPathViolation] {
        &self.violations
    }

    #[must_use]
    pub fn into_violations(self) -> Vec<ChangedPathViolation> {
        self.violations
    }
}

impl fmt::Display for ChangedPathValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} changed path violation(s)",
            self.violations.len()
        )
    }
}

impl std::error::Error for ChangedPathValidationError {}

impl From<DomainError> for DocumentError {
    fn from(error: DomainError) -> Self {
        Self::InvalidDomainValue(error.to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskDocumentStatus {
    Draft,
    Ready,
    WaitingDependency,
    Active,
    Accepted,
    Cancelled,
}

impl TaskDocumentStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "DRAFT",
            Self::Ready => "READY",
            Self::WaitingDependency => "WAITING_DEPENDENCY",
            Self::Active => "ACTIVE",
            Self::Accepted => "ACCEPTED",
            Self::Cancelled => "CANCELLED",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Risk {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentHash(String);

impl ContentHash {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskDocument {
    schema_version: u32,
    id: TaskId,
    title: String,
    status: TaskDocumentStatus,
    contract_revision: ContractRevision,
    acceptance_criteria: Vec<String>,
    dependencies: Vec<TaskDependency>,
    allowed_paths: Vec<String>,
    test_paths: Vec<String>,
    forbidden_paths: Vec<String>,
    risk: Risk,
    body: String,
    content_hash: ContentHash,
}

impl TaskDocument {
    #[must_use]
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    #[must_use]
    pub fn id(&self) -> &TaskId {
        &self.id
    }

    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    #[must_use]
    pub const fn status(&self) -> TaskDocumentStatus {
        self.status
    }

    #[must_use]
    pub const fn contract_revision(&self) -> ContractRevision {
        self.contract_revision
    }

    #[must_use]
    pub fn acceptance_criteria(&self) -> &[String] {
        &self.acceptance_criteria
    }

    #[must_use]
    pub fn dependencies(&self) -> &[TaskDependency] {
        &self.dependencies
    }

    #[must_use]
    pub fn allowed_paths(&self) -> &[String] {
        &self.allowed_paths
    }

    #[must_use]
    pub fn test_paths(&self) -> &[String] {
        &self.test_paths
    }

    #[must_use]
    pub fn forbidden_paths(&self) -> &[String] {
        &self.forbidden_paths
    }

    #[must_use]
    pub const fn risk(&self) -> Risk {
        self.risk
    }

    #[must_use]
    pub fn body(&self) -> &str {
        &self.body
    }

    #[must_use]
    pub const fn content_hash(&self) -> &ContentHash {
        &self.content_hash
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTaskDocument {
    schema_version: u32,
    id: String,
    title: String,
    status: TaskDocumentStatus,
    contract_revision: u64,
    acceptance_criteria: Vec<String>,
    #[serde(default)]
    dependencies: Vec<RawDependency>,
    allowed_paths: Vec<String>,
    #[serde(default)]
    test_paths: Vec<String>,
    #[serde(default)]
    forbidden_paths: Vec<String>,
    risk: Risk,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDependency {
    task_id: String,
    kind: RawDependencyKind,
    reason: String,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RawDependencyKind {
    BlocksStart,
    BlocksIntegration,
}

impl From<RawDependencyKind> for TaskDependencyKind {
    fn from(value: RawDependencyKind) -> Self {
        match value {
            RawDependencyKind::BlocksStart => Self::BlocksStart,
            RawDependencyKind::BlocksIntegration => Self::BlocksIntegration,
        }
    }
}

pub fn load_task_document(source: &str) -> Result<TaskDocument, DocumentError> {
    let normalized = source.replace("\r\n", "\n");
    let rest = normalized
        .strip_prefix("---\n")
        .ok_or(DocumentError::MissingFrontMatter)?;
    let (front_matter, body) = rest
        .split_once("\n---\n")
        .ok_or(DocumentError::MissingFrontMatter)?;
    let raw: RawTaskDocument = serde_saphyr::from_str(front_matter)
        .map_err(|error| DocumentError::InvalidFrontMatter(error.to_string()))?;
    if raw.schema_version != SUPPORTED_SCHEMA_VERSION {
        return Err(DocumentError::UnsupportedSchemaVersion(raw.schema_version));
    }
    if raw.title.trim().is_empty() {
        return Err(DocumentError::InvalidDomainValue(
            "title must not be empty".to_owned(),
        ));
    }
    if raw.acceptance_criteria.is_empty()
        || raw
            .acceptance_criteria
            .iter()
            .any(|criterion| criterion.trim().is_empty())
    {
        return Err(DocumentError::InvalidDomainValue(
            "acceptance_criteria must contain non-empty IDs".to_owned(),
        ));
    }
    validate_paths(&raw.allowed_paths, &raw.test_paths, &raw.forbidden_paths)?;

    let id = TaskId::new(raw.id)?;
    let dependencies = raw
        .dependencies
        .into_iter()
        .map(|dependency| {
            TaskDependency::new(
                id.clone(),
                TaskId::new(dependency.task_id)?,
                dependency.kind.into(),
                dependency.reason,
            )
            .map_err(DocumentError::from)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let digest = Sha256::digest(source.as_bytes());

    Ok(TaskDocument {
        schema_version: raw.schema_version,
        id,
        title: raw.title.trim().to_owned(),
        status: raw.status,
        contract_revision: ContractRevision::new(raw.contract_revision)?,
        acceptance_criteria: raw.acceptance_criteria,
        dependencies,
        allowed_paths: raw.allowed_paths,
        test_paths: raw.test_paths,
        forbidden_paths: raw.forbidden_paths,
        risk: raw.risk,
        body: body.to_owned(),
        content_hash: ContentHash(format!("{digest:x}")),
    })
}

pub fn validate_task_documents(documents: &[TaskDocument]) -> Result<Vec<TaskId>, DocumentError> {
    let tasks = documents
        .iter()
        .map(|document| document.id.clone())
        .collect::<Vec<_>>();
    let dependencies = documents
        .iter()
        .flat_map(|document| document.dependencies.iter().cloned())
        .collect::<Vec<_>>();
    validate_task_graph(&tasks, &dependencies)
        .map_err(|error| DocumentError::InvalidTaskGraph(error.to_string()))
}

/// Validates all changed paths against a task contract.
///
/// Forbidden patterns take priority over allowed and test patterns. Violations
/// are returned in lexicographic path order instead of stopping at the first
/// invalid path.
pub fn validate_changed_paths<I, P>(
    document: &TaskDocument,
    changed_paths: I,
) -> Result<(), ChangedPathValidationError>
where
    I: IntoIterator<Item = P>,
    P: AsRef<str>,
{
    let mut violations = changed_paths
        .into_iter()
        .filter_map(|path| changed_path_violation(document, path.as_ref()))
        .collect::<Vec<_>>();
    violations.sort_by(|left, right| {
        left.path()
            .cmp(right.path())
            .then_with(|| left.sort_order().cmp(&right.sort_order()))
    });

    if violations.is_empty() {
        Ok(())
    } else {
        Err(ChangedPathValidationError { violations })
    }
}

fn changed_path_violation(
    document: &TaskDocument,
    changed_path: &str,
) -> Option<ChangedPathViolation> {
    if !is_safe_relative_path(changed_path) {
        return Some(ChangedPathViolation::InvalidRelativePath {
            path: changed_path.to_owned(),
        });
    }

    if let Some(pattern) = document
        .forbidden_paths
        .iter()
        .filter(|pattern| path_pattern_matches(pattern, changed_path))
        .min()
    {
        return Some(ChangedPathViolation::ForbiddenPath {
            path: changed_path.to_owned(),
            pattern: pattern.clone(),
        });
    }

    let is_allowed = document
        .allowed_paths
        .iter()
        .chain(&document.test_paths)
        .any(|pattern| path_pattern_matches(pattern, changed_path));
    (!is_allowed).then(|| ChangedPathViolation::OutsideAllowedAndTestPaths {
        path: changed_path.to_owned(),
    })
}

fn path_pattern_matches(pattern: &str, path: &str) -> bool {
    let path_segments = path.split('/').collect::<Vec<_>>();
    let mut reachable = vec![false; path_segments.len() + 1];
    reachable[0] = true;

    for pattern_segment in pattern.split('/') {
        let mut next = vec![false; path_segments.len() + 1];
        if pattern_segment == "**" {
            let mut can_match = false;
            for (index, was_reachable) in reachable.iter().copied().enumerate() {
                can_match |= was_reachable;
                next[index] = can_match;
            }
        } else {
            for (index, path_segment) in path_segments.iter().enumerate() {
                next[index + 1] = reachable[index]
                    && (pattern_segment == "*" || pattern_segment == *path_segment);
            }
        }
        reachable = next;
    }

    reachable[path_segments.len()]
}

fn validate_paths(
    allowed_paths: &[String],
    test_paths: &[String],
    forbidden_paths: &[String],
) -> Result<(), DocumentError> {
    if allowed_paths.is_empty() {
        return Err(DocumentError::EmptyAllowedPaths);
    }
    for path in allowed_paths
        .iter()
        .chain(test_paths)
        .chain(forbidden_paths)
    {
        if !is_safe_relative_path(path) {
            return Err(DocumentError::UnsafePath(path.clone()));
        }
    }
    let forbidden = forbidden_paths.iter().collect::<BTreeSet<_>>();
    if let Some(conflict) = allowed_paths
        .iter()
        .chain(test_paths)
        .find(|path| forbidden.contains(path))
    {
        return Err(DocumentError::ConflictingPath(conflict.clone()));
    }
    Ok(())
}

fn is_safe_relative_path(path: &str) -> bool {
    !path.trim().is_empty()
        && !path.starts_with('/')
        && !path.starts_with('\\')
        && !path.contains('\\')
        && !path.contains(':')
        && !path
            .split('/')
            .any(|component| matches!(component, "" | "." | ".."))
}
