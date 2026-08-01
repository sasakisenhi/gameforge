use std::collections::{BTreeMap, BTreeSet};

use crate::{ContractRevision, DomainError, TaskId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    Draft,
    Ready,
    WaitingDependency,
    Active,
    Accepted,
    Cancelled,
}

impl TaskStatus {
    const fn name(self) -> &'static str {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    id: TaskId,
    status: TaskStatus,
    contract_revision: Option<ContractRevision>,
}

impl Task {
    #[must_use]
    pub const fn draft(id: TaskId) -> Self {
        Self {
            id,
            status: TaskStatus::Draft,
            contract_revision: None,
        }
    }

    #[must_use]
    pub fn id(&self) -> &TaskId {
        &self.id
    }

    #[must_use]
    pub const fn status(&self) -> TaskStatus {
        self.status
    }

    #[must_use]
    pub const fn is_ready(&self) -> bool {
        matches!(self.status, TaskStatus::Ready)
    }

    #[must_use]
    pub const fn contract_revision(&self) -> Option<ContractRevision> {
        self.contract_revision
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskCommand {
    MarkReady { contract_revision: ContractRevision },
    WaitForDependency,
    DependencySatisfied,
    Activate,
    MarkAcceptedAfterMerge,
    Cancel,
}

impl TaskCommand {
    const fn name(&self) -> &'static str {
        match self {
            Self::MarkReady { .. } => "MarkReady",
            Self::WaitForDependency => "WaitForDependency",
            Self::DependencySatisfied => "DependencySatisfied",
            Self::Activate => "Activate",
            Self::MarkAcceptedAfterMerge => "MarkAcceptedAfterMerge",
            Self::Cancel => "Cancel",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskEvent {
    Ready { contract_revision: ContractRevision },
    DependencyWaitStarted,
    DependencySatisfied,
    Activated,
    AcceptedAfterMerge,
    Cancelled,
}

pub fn decide_task(task: &Task, command: TaskCommand) -> Result<Vec<TaskEvent>, DomainError> {
    let event = match (&task.status, &command) {
        (TaskStatus::Draft, TaskCommand::MarkReady { contract_revision }) => TaskEvent::Ready {
            contract_revision: *contract_revision,
        },
        (TaskStatus::Ready, TaskCommand::WaitForDependency) => TaskEvent::DependencyWaitStarted,
        (TaskStatus::WaitingDependency, TaskCommand::DependencySatisfied) => {
            TaskEvent::DependencySatisfied
        }
        (TaskStatus::Ready, TaskCommand::Activate) => TaskEvent::Activated,
        (TaskStatus::Active, TaskCommand::MarkAcceptedAfterMerge) => TaskEvent::AcceptedAfterMerge,
        (
            TaskStatus::Draft
            | TaskStatus::Ready
            | TaskStatus::WaitingDependency
            | TaskStatus::Active,
            TaskCommand::Cancel,
        ) => TaskEvent::Cancelled,
        _ => {
            return Err(DomainError::InvalidTransition {
                aggregate: "Task",
                from: task.status.name(),
                command: command.name(),
            });
        }
    };
    Ok(vec![event])
}

#[must_use]
pub fn evolve_task(mut task: Task, event: &TaskEvent) -> Task {
    match event {
        TaskEvent::Ready { contract_revision } => {
            task.status = TaskStatus::Ready;
            task.contract_revision = Some(*contract_revision);
        }
        TaskEvent::DependencyWaitStarted => task.status = TaskStatus::WaitingDependency,
        TaskEvent::DependencySatisfied => task.status = TaskStatus::Ready,
        TaskEvent::Activated => task.status = TaskStatus::Active,
        TaskEvent::AcceptedAfterMerge => task.status = TaskStatus::Accepted,
        TaskEvent::Cancelled => task.status = TaskStatus::Cancelled,
    }
    task
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskDependencyKind {
    BlocksStart,
    BlocksIntegration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskDependency {
    task_id: TaskId,
    depends_on: TaskId,
    kind: TaskDependencyKind,
    reason: String,
}

impl TaskDependency {
    pub fn new(
        task_id: TaskId,
        depends_on: TaskId,
        kind: TaskDependencyKind,
        reason: impl Into<String>,
    ) -> Result<Self, DomainError> {
        if task_id == depends_on {
            return Err(DomainError::SelfDependency(task_id));
        }
        let reason = reason.into();
        if reason.trim().is_empty() {
            return Err(DomainError::EmptyDependencyReason);
        }
        Ok(Self {
            task_id,
            depends_on,
            kind,
            reason: reason.trim().to_owned(),
        })
    }

    #[must_use]
    pub fn task_id(&self) -> &TaskId {
        &self.task_id
    }

    #[must_use]
    pub fn depends_on(&self) -> &TaskId {
        &self.depends_on
    }

    #[must_use]
    pub const fn kind(&self) -> TaskDependencyKind {
        self.kind
    }

    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

/// DAGを検証し、依存先を先にした決定的な順序を返す。
pub fn validate_task_graph(
    tasks: &[TaskId],
    dependencies: &[TaskDependency],
) -> Result<Vec<TaskId>, DomainError> {
    let mut all = BTreeSet::new();
    for task in tasks {
        if !all.insert(task.clone()) {
            return Err(DomainError::DuplicateTask(task.clone()));
        }
    }

    let mut indegree: BTreeMap<TaskId, usize> = all.iter().cloned().map(|task| (task, 0)).collect();
    let mut dependents: BTreeMap<TaskId, BTreeSet<TaskId>> = BTreeMap::new();

    for dependency in dependencies {
        if !all.contains(&dependency.task_id) {
            return Err(DomainError::MissingDependency {
                task_id: dependency.task_id.clone(),
                depends_on: dependency.task_id.clone(),
            });
        }
        if !all.contains(&dependency.depends_on) {
            return Err(DomainError::MissingDependency {
                task_id: dependency.task_id.clone(),
                depends_on: dependency.depends_on.clone(),
            });
        }
        let inserted = dependents
            .entry(dependency.depends_on.clone())
            .or_default()
            .insert(dependency.task_id.clone());
        if inserted {
            let Some(count) = indegree.get_mut(&dependency.task_id) else {
                return Err(DomainError::MissingDependency {
                    task_id: dependency.task_id.clone(),
                    depends_on: dependency.task_id.clone(),
                });
            };
            *count += 1;
        }
    }

    let mut ready: BTreeSet<TaskId> = indegree
        .iter()
        .filter(|(_, count)| **count == 0)
        .map(|(task, _)| task.clone())
        .collect();
    let mut order = Vec::with_capacity(all.len());

    while let Some(task) = ready.pop_first() {
        order.push(task.clone());
        if let Some(next_tasks) = dependents.get(&task) {
            for next in next_tasks {
                let Some(count) = indegree.get_mut(next) else {
                    return Err(DomainError::MissingDependency {
                        task_id: next.clone(),
                        depends_on: task.clone(),
                    });
                };
                *count -= 1;
                if *count == 0 {
                    ready.insert(next.clone());
                }
            }
        }
    }

    if order.len() != all.len() {
        let cycle = indegree
            .into_iter()
            .filter(|(_, count)| *count > 0)
            .map(|(task, _)| task)
            .collect();
        return Err(DomainError::DependencyCycle(cycle));
    }
    Ok(order)
}
