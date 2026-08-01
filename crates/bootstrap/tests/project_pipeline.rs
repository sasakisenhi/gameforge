use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use gameforge_application::ApplicationCommand;
use gameforge_bootstrap::{
    BootstrapError, CommandContext, rebuild_project, start_project, validate_project,
};
use gameforge_runtime::ScheduleConfig;

static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

struct TempProject(PathBuf);

impl TempProject {
    fn create() -> Self {
        let unique = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "gameforge-bootstrap-{}-{unique}",
            std::process::id()
        ));
        let tasks = root.join(".game-dev/tasks");
        fs::create_dir_all(&tasks).unwrap();
        fs::write(tasks.join("TASK-001.md"), TASK).unwrap();
        Self(root)
    }

    fn create_with_two_tasks() -> Self {
        let project = Self::create();
        fs::write(project.0.join(".game-dev/tasks/TASK-002.md"), TASK_2).unwrap();
        project
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

const TASK: &str = r"---
schema_version: 1
id: TASK-001
title: 砂の落下規則
status: ready
contract_revision: 1
acceptance_criteria:
  - AC-001
dependencies: []
allowed_paths:
  - crates/game_logic/src/sand/**
test_paths:
  - crates/game_logic/tests/sand/**
forbidden_paths:
  - crates/game_runtime/**
risk: low
---

# 目的

砂を落下させる。
";

const TASK_2: &str = r"---
schema_version: 1
id: TASK-002
title: 砂の描画規則
status: ready
contract_revision: 1
acceptance_criteria:
  - AC-002
dependencies: []
allowed_paths:
  - crates/game_render/src/sand/**
test_paths:
  - crates/game_render/tests/sand/**
forbidden_paths:
  - crates/game_runtime/**
risk: low
---

# 目的

砂を描画する。
";

#[test]
fn validates_and_rebuilds_a_project_through_one_composition_root() {
    let project = TempProject::create();
    let validation = validate_project(project.path()).unwrap();
    assert_eq!(validation.task_count, 1);
    assert_eq!(validation.integration_order, ["TASK-001"]);

    let snapshot = rebuild_project(project.path(), "test-coordinator").unwrap();
    assert_eq!(snapshot.projection_revision, 0);
    assert_eq!(snapshot.development_board.len(), 1);
    assert_eq!(snapshot.development_board[0].task_id, "TASK-001");
    assert!(project.path().join(".game-dev/read-model.sqlite").is_file());
}

#[test]
fn project_session_holds_the_single_writer_lease_until_it_is_dropped() {
    let project = TempProject::create();
    let session = start_project(project.path(), "desktop-coordinator").unwrap();

    assert_eq!(session.snapshot().development_board.len(), 1);
    let error = start_project(project.path(), "other-coordinator")
        .err()
        .expect("a second writer must be rejected");
    assert!(error.to_string().contains("already owned"));

    drop(session);
    start_project(project.path(), "other-coordinator").unwrap();
}

#[test]
fn queue_command_records_a_run_updates_the_board_and_survives_restart() {
    let project = TempProject::create();
    let mut session = start_project(project.path(), "desktop-coordinator").unwrap();

    let snapshot = session
        .execute(
            command_context("CMD-001"),
            ApplicationCommand::QueueTaskRun {
                task_id: "TASK-001".to_owned(),
                expected_projection_revision: 0,
            },
        )
        .unwrap();

    assert_eq!(snapshot.projection_revision, 1);
    assert_eq!(
        snapshot.development_board[0].current_run_id.as_deref(),
        Some("RUN-TASK-001-1")
    );
    assert_eq!(
        snapshot.development_board[0].run_status.as_deref(),
        Some("QUEUED")
    );
    let journal = fs::read_to_string(project.path().join(".game-dev/events/events.jsonl")).unwrap();
    assert!(journal.contains("TaskRunQueued"));
    assert!(journal.contains("\"contract_revision\":\"1\""));
    assert!(journal.contains(&format!("\"base_commit\":\"{}\"", "a".repeat(40))));

    drop(session);
    let restarted = start_project(project.path(), "restarted-coordinator").unwrap();
    assert_eq!(restarted.snapshot(), &snapshot);
}

#[test]
fn retrying_the_same_command_does_not_duplicate_the_run_or_event() {
    let project = TempProject::create();
    let mut session = start_project(project.path(), "desktop-coordinator").unwrap();
    let command = ApplicationCommand::QueueTaskRun {
        task_id: "TASK-001".to_owned(),
        expected_projection_revision: 0,
    };

    let first = session
        .execute(command_context("CMD-001"), command.clone())
        .unwrap();
    drop(session);
    let mut restarted = start_project(project.path(), "restarted-coordinator").unwrap();
    let retried = restarted
        .execute(command_context("CMD-001"), command)
        .unwrap();

    assert_eq!(retried, first);
    assert_eq!(retried.projection_revision, 1);
    let journal = fs::read_to_string(project.path().join(".game-dev/events/events.jsonl")).unwrap();
    assert_eq!(journal.lines().count(), 1);
}

#[test]
fn reusing_a_command_id_with_different_content_is_rejected() {
    let project = TempProject::create();
    let mut session = start_project(project.path(), "desktop-coordinator").unwrap();
    let command = ApplicationCommand::QueueTaskRun {
        task_id: "TASK-001".to_owned(),
        expected_projection_revision: 0,
    };
    session
        .execute(command_context("CMD-001"), command.clone())
        .unwrap();

    let mut changed_context = command_context("CMD-001");
    changed_context.base_commit = "b".repeat(40);
    assert_eq!(
        session.execute(changed_context, command).unwrap_err(),
        BootstrapError::CommandIdConflict("CMD-001".to_owned())
    );
    assert_eq!(session.snapshot().projection_revision, 1);
    let journal = fs::read_to_string(project.path().join(".game-dev/events/events.jsonl")).unwrap();
    assert_eq!(journal.lines().count(), 1);
}

#[test]
fn stale_projection_or_already_queued_task_is_rejected_without_mutation() {
    let project = TempProject::create();
    let mut session = start_project(project.path(), "desktop-coordinator").unwrap();

    let stale = session
        .execute(
            command_context("CMD-STALE"),
            ApplicationCommand::QueueTaskRun {
                task_id: "TASK-001".to_owned(),
                expected_projection_revision: 9,
            },
        )
        .unwrap_err();
    assert_eq!(
        stale,
        BootstrapError::ProjectionRevisionConflict {
            expected: 9,
            actual: 0,
        }
    );
    assert_eq!(session.snapshot().projection_revision, 0);

    session
        .execute(
            command_context("CMD-001"),
            ApplicationCommand::QueueTaskRun {
                task_id: "TASK-001".to_owned(),
                expected_projection_revision: 0,
            },
        )
        .unwrap();
    let already_queued = session
        .execute(
            command_context("CMD-002"),
            ApplicationCommand::QueueTaskRun {
                task_id: "TASK-001".to_owned(),
                expected_projection_revision: 1,
            },
        )
        .unwrap_err();
    assert_eq!(
        already_queued,
        BootstrapError::TaskAlreadyHasRun("TASK-001".to_owned())
    );
    assert_eq!(session.snapshot().projection_revision, 1);
    let journal = fs::read_to_string(project.path().join(".game-dev/events/events.jsonl")).unwrap();
    assert_eq!(journal.lines().count(), 1);
}

#[test]
fn scheduler_starts_only_the_first_queued_run_and_survives_restart() {
    let project = TempProject::create_with_two_tasks();
    let mut session = start_project(project.path(), "desktop-coordinator").unwrap();
    queue(&mut session, "CMD-QUEUE-1", "TASK-001", 0);
    queue(&mut session, "CMD-QUEUE-2", "TASK-002", 1);

    let scheduled = session
        .run_scheduler(
            command_context("CMD-SCHEDULER-1"),
            ScheduleConfig {
                max_concurrent_task_runs: 1,
            },
        )
        .unwrap();

    assert_eq!(scheduled.projection_revision, 3);
    assert_eq!(
        scheduled.development_board[0].run_status.as_deref(),
        Some("PREPARING")
    );
    assert_eq!(
        scheduled.development_board[1].run_status.as_deref(),
        Some("QUEUED")
    );
    let journal = fs::read_to_string(project.path().join(".game-dev/events/events.jsonl")).unwrap();
    assert_eq!(journal.lines().count(), 3);
    assert!(journal.contains("TaskRunStateChanged"));
    assert!(journal.contains("\"state\":\"PREPARING\""));

    drop(session);
    let mut restarted = start_project(project.path(), "restarted-coordinator").unwrap();
    assert_eq!(restarted.snapshot(), &scheduled);
    assert_eq!(
        restarted
            .run_scheduler(
                command_context("CMD-SCHEDULER-1"),
                ScheduleConfig {
                    max_concurrent_task_runs: 1,
                },
            )
            .unwrap(),
        scheduled
    );
    let journal = fs::read_to_string(project.path().join(".game-dev/events/events.jsonl")).unwrap();
    assert_eq!(journal.lines().count(), 3);
}

#[test]
fn scheduler_at_capacity_keeps_the_second_run_queued_without_new_events() {
    let project = TempProject::create_with_two_tasks();
    let mut session = start_project(project.path(), "desktop-coordinator").unwrap();
    queue(&mut session, "CMD-QUEUE-1", "TASK-001", 0);
    queue(&mut session, "CMD-QUEUE-2", "TASK-002", 1);
    session
        .run_scheduler(
            command_context("CMD-SCHEDULER-1"),
            ScheduleConfig {
                max_concurrent_task_runs: 1,
            },
        )
        .unwrap();

    let unchanged = session
        .run_scheduler(
            command_context("CMD-SCHEDULER-2"),
            ScheduleConfig {
                max_concurrent_task_runs: 1,
            },
        )
        .unwrap();

    assert_eq!(unchanged.projection_revision, 3);
    assert_eq!(
        unchanged.development_board[1].run_status.as_deref(),
        Some("QUEUED")
    );
    let journal = fs::read_to_string(project.path().join(".game-dev/events/events.jsonl")).unwrap();
    assert_eq!(journal.lines().count(), 3);
}

fn queue(
    session: &mut gameforge_bootstrap::ProjectSession,
    command_id: &str,
    task_id: &str,
    expected_projection_revision: u64,
) {
    session
        .execute(
            command_context(command_id),
            ApplicationCommand::QueueTaskRun {
                task_id: task_id.to_owned(),
                expected_projection_revision,
            },
        )
        .unwrap();
}

fn command_context(command_id: &str) -> CommandContext {
    CommandContext {
        command_id: command_id.to_owned(),
        actor: "desktop-test".to_owned(),
        occurred_at: "2026-08-01T12:00:00+09:00".to_owned(),
        base_commit: "a".repeat(40),
    }
}
