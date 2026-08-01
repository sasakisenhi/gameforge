#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartBlocker {
    Dependency,
    Conflict,
    Stale,
    HumanApproval,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedRun {
    pub task_run_id: String,
    pub priority: i32,
    pub queued_at: String,
    pub start_blocker: Option<StartBlocker>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleSnapshot {
    pub queued: Vec<QueuedRun>,
    pub running: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScheduleConfig {
    pub max_concurrent_task_runs: usize,
}

pub const DEFAULT_MAX_CONCURRENT_TASK_RUNS: usize = 3;

impl Default for ScheduleConfig {
    fn default() -> Self {
        Self {
            max_concurrent_task_runs: DEFAULT_MAX_CONCURRENT_TASK_RUNS,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeferralReason {
    Capacity,
    Blocked(StartBlocker),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeferredRun {
    pub task_run_id: String,
    pub reason: DeferralReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulePlan {
    pub start: Vec<String>,
    pub keep_running: Vec<String>,
    pub defer: Vec<DeferredRun>,
}

#[must_use]
pub fn plan_schedule(snapshot: &ScheduleSnapshot, config: &ScheduleConfig) -> SchedulePlan {
    let mut keep_running = snapshot.running.clone();
    keep_running.sort();
    let available_capacity = config
        .max_concurrent_task_runs
        .saturating_sub(keep_running.len());

    let mut queued = snapshot.queued.clone();
    queued.sort_by(|left, right| {
        right
            .priority
            .cmp(&left.priority)
            .then_with(|| left.queued_at.cmp(&right.queued_at))
            .then_with(|| left.task_run_id.cmp(&right.task_run_id))
    });

    let mut start = Vec::new();
    let mut defer = Vec::new();
    for run in queued {
        if let Some(blocker) = run.start_blocker {
            defer.push(DeferredRun {
                task_run_id: run.task_run_id,
                reason: DeferralReason::Blocked(blocker),
            });
        } else if start.len() < available_capacity {
            start.push(run.task_run_id);
        } else {
            defer.push(DeferredRun {
                task_run_id: run.task_run_id,
                reason: DeferralReason::Capacity,
            });
        }
    }

    SchedulePlan {
        start,
        keep_running,
        defer,
    }
}
