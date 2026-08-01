use gameforge_runtime::{
    DeferralReason, DeferredRun, QueuedRun, ScheduleConfig, ScheduleSnapshot, StartBlocker,
    plan_schedule,
};

fn queued(run_id: &str, priority: i32, queued_at: &str) -> QueuedRun {
    QueuedRun {
        task_run_id: run_id.to_owned(),
        priority,
        queued_at: queued_at.to_owned(),
        start_blocker: None,
    }
}

#[test]
fn starts_only_one_run_in_deterministic_priority_queue_order() {
    let snapshot = ScheduleSnapshot {
        queued: vec![
            queued("RUN-LOW", 0, "2026-08-01T10:00:00+09:00"),
            queued("RUN-HIGH-B", 10, "2026-08-01T10:02:00+09:00"),
            queued("RUN-HIGH-A", 10, "2026-08-01T10:01:00+09:00"),
        ],
        running: Vec::new(),
    };

    let plan = plan_schedule(
        &snapshot,
        &ScheduleConfig {
            max_concurrent_task_runs: 1,
        },
    );

    assert_eq!(plan.start, ["RUN-HIGH-A"]);
    assert!(plan.keep_running.is_empty());
    assert_eq!(
        plan.defer,
        [
            DeferredRun {
                task_run_id: "RUN-HIGH-B".to_owned(),
                reason: DeferralReason::Capacity,
            },
            DeferredRun {
                task_run_id: "RUN-LOW".to_owned(),
                reason: DeferralReason::Capacity,
            },
        ]
    );
}

#[test]
fn keeps_the_running_run_and_does_not_start_another_at_capacity() {
    let snapshot = ScheduleSnapshot {
        queued: vec![queued("RUN-QUEUED", 0, "2026-08-01T10:00:00+09:00")],
        running: vec!["RUN-ACTIVE".to_owned()],
    };

    let plan = plan_schedule(
        &snapshot,
        &ScheduleConfig {
            max_concurrent_task_runs: 1,
        },
    );

    assert!(plan.start.is_empty());
    assert_eq!(plan.keep_running, ["RUN-ACTIVE"]);
    assert_eq!(
        plan.defer,
        [DeferredRun {
            task_run_id: "RUN-QUEUED".to_owned(),
            reason: DeferralReason::Capacity,
        }]
    );
}

#[test]
fn blocked_run_does_not_consume_capacity_needed_by_the_next_eligible_run() {
    let mut blocked = queued("RUN-BLOCKED", 10, "2026-08-01T10:00:00+09:00");
    blocked.start_blocker = Some(StartBlocker::Dependency);
    let snapshot = ScheduleSnapshot {
        queued: vec![
            blocked,
            queued("RUN-ELIGIBLE", 0, "2026-08-01T10:01:00+09:00"),
        ],
        running: Vec::new(),
    };

    let plan = plan_schedule(
        &snapshot,
        &ScheduleConfig {
            max_concurrent_task_runs: 1,
        },
    );

    assert_eq!(plan.start, ["RUN-ELIGIBLE"]);
    assert_eq!(
        plan.defer,
        [DeferredRun {
            task_run_id: "RUN-BLOCKED".to_owned(),
            reason: DeferralReason::Blocked(StartBlocker::Dependency),
        }]
    );
}

#[test]
fn default_schedule_capacity_starts_three_runs() {
    let snapshot = ScheduleSnapshot {
        queued: vec![
            queued("RUN-1", 0, "2026-08-01T10:00:00+09:00"),
            queued("RUN-2", 0, "2026-08-01T10:01:00+09:00"),
            queued("RUN-3", 0, "2026-08-01T10:02:00+09:00"),
            queued("RUN-4", 0, "2026-08-01T10:03:00+09:00"),
        ],
        running: Vec::new(),
    };

    let plan = plan_schedule(&snapshot, &ScheduleConfig::default());

    assert_eq!(plan.start, ["RUN-1", "RUN-2", "RUN-3"]);
    assert_eq!(plan.defer.len(), 1);
    assert_eq!(plan.defer[0].task_run_id, "RUN-4");
}
