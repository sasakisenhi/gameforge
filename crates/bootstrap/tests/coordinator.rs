use std::{
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use gameforge_application::{
    ApplicationCommand, LocalVerificationPort, LocalVerificationRequest, LocalVerificationUpdate,
    RunExecutionPort, RunExecutionUpdate, RunLaunchOutcome, RunLaunchRequest, StartedRun,
};
use gameforge_bootstrap::{CommandContext, ProjectCoordinator, start_project};
use gameforge_runtime::ScheduleConfig;

static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

struct TempProject(PathBuf);

impl TempProject {
    fn with_tasks(count: usize) -> Self {
        let unique = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "gameforge-coordinator-{}-{unique}",
            std::process::id()
        ));
        let tasks = root.join(".game-dev/tasks");
        fs::create_dir_all(&tasks).unwrap();
        for number in 1..=count {
            let id = format!("TASK-{number:03}");
            let document = format!(
                "---\nschema_version: 1\nid: {id}\ntitle: Task {number}\nstatus: ready\ncontract_revision: 1\nacceptance_criteria:\n  - AC-{number:03}\ndependencies: []\nallowed_paths:\n  - crates/task_{number}/src/**\ntest_paths:\n  - crates/task_{number}/tests/**\nforbidden_paths: []\nrisk: low\n---\n\nImplement task {number}.\n"
            );
            fs::write(tasks.join(format!("{id}.md")), document).unwrap();
        }
        Self(root)
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

#[derive(Default)]
struct FakeState {
    started: Vec<String>,
    cancelled: Vec<String>,
    updates: VecDeque<RunExecutionUpdate>,
    verification_started: Vec<LocalVerificationRequest>,
    verification_cancelled: Vec<String>,
    verification_updates: VecDeque<LocalVerificationUpdate>,
}

struct FakeExecution {
    state: Arc<Mutex<FakeState>>,
}

struct FakeVerification {
    state: Arc<Mutex<FakeState>>,
}

impl RunExecutionPort for FakeExecution {
    fn start_run(&mut self, request: &RunLaunchRequest) -> RunLaunchOutcome {
        self.state
            .lock()
            .unwrap()
            .started
            .push(request.task_run_id.clone());
        RunLaunchOutcome::Started(
            StartedRun::new(
                format!("resource-{}", request.task_run_id),
                format!("worktree-{}", request.task_run_id),
                format!("agent-{}", request.task_run_id),
            )
            .unwrap(),
        )
    }

    fn poll_updates(&mut self) -> Vec<RunExecutionUpdate> {
        self.state.lock().unwrap().updates.drain(..).collect()
    }

    fn cancel_run(&mut self, task_run_id: &str) -> Result<(), String> {
        self.state
            .lock()
            .unwrap()
            .cancelled
            .push(task_run_id.to_owned());
        Ok(())
    }
}

impl LocalVerificationPort for FakeVerification {
    fn start_verification(&mut self, request: &LocalVerificationRequest) -> Result<(), String> {
        self.state
            .lock()
            .unwrap()
            .verification_started
            .push(request.clone());
        Ok(())
    }

    fn poll_updates(&mut self) -> Vec<LocalVerificationUpdate> {
        self.state
            .lock()
            .unwrap()
            .verification_updates
            .drain(..)
            .collect()
    }

    fn cancel_verification(&mut self, task_run_id: &str) -> Result<(), String> {
        self.state
            .lock()
            .unwrap()
            .verification_cancelled
            .push(task_run_id.to_owned());
        Ok(())
    }
}

#[test]
fn completion_releases_capacity_and_starts_the_next_queued_run() {
    let project = TempProject::with_tasks(4);
    let state = Arc::new(Mutex::new(FakeState::default()));
    let execution = FakeExecution {
        state: Arc::clone(&state),
    };
    let verification = FakeVerification {
        state: Arc::clone(&state),
    };
    let session = start_project(project.path(), "coordinator").unwrap();
    let mut coordinator = ProjectCoordinator::with_verification(
        session,
        execution,
        verification,
        ScheduleConfig::default(),
    );

    for number in 1..=4 {
        let revision = coordinator.snapshot().projection_revision;
        coordinator
            .execute(
                &context(&format!("queue-{number}")),
                ApplicationCommand::QueueTaskRun {
                    task_id: format!("TASK-{number:03}"),
                    expected_projection_revision: revision,
                },
            )
            .unwrap();
    }
    assert_eq!(
        state.lock().unwrap().started,
        ["RUN-TASK-001-1", "RUN-TASK-002-1", "RUN-TASK-003-1"]
    );

    state
        .lock()
        .unwrap()
        .updates
        .push_back(RunExecutionUpdate::Completed {
            task_run_id: "RUN-TASK-001-1".to_owned(),
            agent_session_id: "agent-RUN-TASK-001-1".to_owned(),
            red_evidence_present: true,
            green_evidence_present: true,
        });
    let snapshot = coordinator.tick(&context("completion-tick")).unwrap();

    assert_eq!(
        snapshot.development_board[0].run_status.as_deref(),
        Some("LOCAL_CHECKING")
    );
    assert_eq!(
        snapshot.development_board[3].run_status.as_deref(),
        Some("AGENT_RUNNING")
    );
    assert_eq!(state.lock().unwrap().started.len(), 4);
    assert_eq!(
        state.lock().unwrap().verification_started,
        [LocalVerificationRequest {
            task_run_id: "RUN-TASK-001-1".to_owned(),
            task_id: "TASK-001".to_owned(),
            base_commit: "a".repeat(40),
            worktree_path: "worktree-RUN-TASK-001-1".to_owned(),
        }]
    );
}

#[test]
fn cancellation_stops_the_process_and_refills_capacity() {
    let project = TempProject::with_tasks(4);
    let state = Arc::new(Mutex::new(FakeState::default()));
    let execution = FakeExecution {
        state: Arc::clone(&state),
    };
    let verification = FakeVerification {
        state: Arc::clone(&state),
    };
    let session = start_project(project.path(), "coordinator").unwrap();
    let mut coordinator = ProjectCoordinator::with_verification(
        session,
        execution,
        verification,
        ScheduleConfig::default(),
    );
    for number in 1..=4 {
        let revision = coordinator.snapshot().projection_revision;
        coordinator
            .execute(
                &context(&format!("queue-{number}")),
                ApplicationCommand::QueueTaskRun {
                    task_id: format!("TASK-{number:03}"),
                    expected_projection_revision: revision,
                },
            )
            .unwrap();
    }

    let revision = coordinator.snapshot().projection_revision;
    let snapshot = coordinator
        .execute(
            &context("cancel-1"),
            ApplicationCommand::CancelTaskRun {
                task_run_id: "RUN-TASK-001-1".to_owned(),
                expected_projection_revision: revision,
            },
        )
        .unwrap();

    assert_eq!(state.lock().unwrap().cancelled, ["RUN-TASK-001-1"]);
    assert_eq!(
        state.lock().unwrap().verification_cancelled,
        ["RUN-TASK-001-1"]
    );
    assert_eq!(
        snapshot.development_board[3].run_status.as_deref(),
        Some("AGENT_RUNNING")
    );
}

#[test]
fn agent_failure_releases_capacity_and_refills_the_queue() {
    let project = TempProject::with_tasks(4);
    let state = Arc::new(Mutex::new(FakeState::default()));
    let execution = FakeExecution {
        state: Arc::clone(&state),
    };
    let verification = FakeVerification {
        state: Arc::clone(&state),
    };
    let session = start_project(project.path(), "coordinator").unwrap();
    let mut coordinator = ProjectCoordinator::with_verification(
        session,
        execution,
        verification,
        ScheduleConfig::default(),
    );
    for number in 1..=4 {
        let revision = coordinator.snapshot().projection_revision;
        coordinator
            .execute(
                &context(&format!("queue-{number}")),
                ApplicationCommand::QueueTaskRun {
                    task_id: format!("TASK-{number:03}"),
                    expected_projection_revision: revision,
                },
            )
            .unwrap();
    }

    state
        .lock()
        .unwrap()
        .updates
        .push_back(RunExecutionUpdate::Failed {
            task_run_id: "RUN-TASK-001-1".to_owned(),
            detail: "Codex process exited".to_owned(),
        });
    let snapshot = coordinator.tick(&context("failure-tick")).unwrap();

    assert_eq!(
        snapshot.development_board[0].run_status.as_deref(),
        Some("FAILED")
    );
    assert_eq!(
        snapshot.development_board[3].run_status.as_deref(),
        Some("AGENT_RUNNING")
    );
    assert_eq!(state.lock().unwrap().started.len(), 4);
}

#[test]
fn passing_local_verification_records_the_output_commit() {
    let project = TempProject::with_tasks(1);
    let state = Arc::new(Mutex::new(FakeState::default()));
    let session = start_project(project.path(), "coordinator").unwrap();
    let mut coordinator = ProjectCoordinator::with_verification(
        session,
        FakeExecution {
            state: Arc::clone(&state),
        },
        FakeVerification {
            state: Arc::clone(&state),
        },
        ScheduleConfig::default(),
    );
    coordinator
        .execute(
            &context("queue-success"),
            ApplicationCommand::QueueTaskRun {
                task_id: "TASK-001".to_owned(),
                expected_projection_revision: 0,
            },
        )
        .unwrap();
    state
        .lock()
        .unwrap()
        .updates
        .push_back(RunExecutionUpdate::Completed {
            task_run_id: "RUN-TASK-001-1".to_owned(),
            agent_session_id: "agent-1".to_owned(),
            red_evidence_present: true,
            green_evidence_present: false,
        });
    coordinator.tick(&context("agent-success")).unwrap();
    state
        .lock()
        .unwrap()
        .verification_updates
        .push_back(LocalVerificationUpdate::Passed {
            task_run_id: "RUN-TASK-001-1".to_owned(),
            head_commit: "b".repeat(40),
            changed_paths: vec!["crates/task_1/src/lib.rs".to_owned()],
            completed_checks: vec!["test".to_owned()],
            final_suite_passed: true,
        });

    let snapshot = coordinator.tick(&context("checks-success")).unwrap();

    assert_eq!(
        snapshot.development_board[0].run_status.as_deref(),
        Some("SUCCEEDED")
    );
    drop(coordinator);
    let journal = fs::read_to_string(project.path().join(".game-dev/events/events.jsonl")).unwrap();
    assert!(journal.contains(&format!("\"head_commit\":\"{}\"", "b".repeat(40))));
}

#[test]
fn scope_violation_fails_verification_and_surfaces_a_health_flag() {
    let project = TempProject::with_tasks(1);
    let state = Arc::new(Mutex::new(FakeState::default()));
    let session = start_project(project.path(), "coordinator").unwrap();
    let mut coordinator = ProjectCoordinator::with_verification(
        session,
        FakeExecution {
            state: Arc::clone(&state),
        },
        FakeVerification {
            state: Arc::clone(&state),
        },
        ScheduleConfig::default(),
    );
    coordinator
        .execute(
            &context("queue-scope"),
            ApplicationCommand::QueueTaskRun {
                task_id: "TASK-001".to_owned(),
                expected_projection_revision: 0,
            },
        )
        .unwrap();
    state
        .lock()
        .unwrap()
        .updates
        .push_back(RunExecutionUpdate::Completed {
            task_run_id: "RUN-TASK-001-1".to_owned(),
            agent_session_id: "agent-1".to_owned(),
            red_evidence_present: true,
            green_evidence_present: true,
        });
    coordinator.tick(&context("agent-scope")).unwrap();
    state
        .lock()
        .unwrap()
        .verification_updates
        .push_back(LocalVerificationUpdate::Passed {
            task_run_id: "RUN-TASK-001-1".to_owned(),
            head_commit: "b".repeat(40),
            changed_paths: vec!["crates/outside/src/lib.rs".to_owned()],
            completed_checks: vec!["test".to_owned()],
            final_suite_passed: true,
        });

    let snapshot = coordinator.tick(&context("checks-scope")).unwrap();

    assert_eq!(
        snapshot.development_board[0].run_status.as_deref(),
        Some("FAILED")
    );
    assert_eq!(
        snapshot.development_board[0].health_flags,
        ["SCOPE_VIOLATION"]
    );
    drop(coordinator);
    let restarted = start_project(project.path(), "coordinator-restarted").unwrap();
    assert_eq!(
        restarted.snapshot().development_board[0].health_flags,
        ["SCOPE_VIOLATION"]
    );
}

#[test]
fn behavior_change_without_red_evidence_fails_the_tdd_gate() {
    let project = TempProject::with_tasks(1);
    let state = Arc::new(Mutex::new(FakeState::default()));
    let session = start_project(project.path(), "coordinator").unwrap();
    let mut coordinator = ProjectCoordinator::with_verification(
        session,
        FakeExecution {
            state: Arc::clone(&state),
        },
        FakeVerification {
            state: Arc::clone(&state),
        },
        ScheduleConfig::default(),
    );
    coordinator
        .execute(
            &context("queue-tdd"),
            ApplicationCommand::QueueTaskRun {
                task_id: "TASK-001".to_owned(),
                expected_projection_revision: 0,
            },
        )
        .unwrap();
    state
        .lock()
        .unwrap()
        .updates
        .push_back(RunExecutionUpdate::Completed {
            task_run_id: "RUN-TASK-001-1".to_owned(),
            agent_session_id: "agent-1".to_owned(),
            red_evidence_present: false,
            green_evidence_present: true,
        });
    coordinator.tick(&context("agent-tdd")).unwrap();
    state
        .lock()
        .unwrap()
        .verification_updates
        .push_back(LocalVerificationUpdate::Passed {
            task_run_id: "RUN-TASK-001-1".to_owned(),
            head_commit: "b".repeat(40),
            changed_paths: vec!["crates/task_1/src/lib.rs".to_owned()],
            completed_checks: vec!["test".to_owned()],
            final_suite_passed: true,
        });

    let snapshot = coordinator.tick(&context("checks-tdd")).unwrap();

    assert_eq!(
        snapshot.development_board[0].run_status.as_deref(),
        Some("FAILED")
    );
    assert_eq!(
        snapshot.development_board[0].health_flags,
        ["TDD_SEQUENCE_VIOLATION"]
    );
}

fn context(command_id: &str) -> CommandContext {
    CommandContext {
        command_id: command_id.to_owned(),
        actor: "coordinator-test".to_owned(),
        occurred_at: "2026-08-01T12:00:00+09:00".to_owned(),
        base_commit: "a".repeat(40),
    }
}
