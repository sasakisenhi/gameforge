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
    ApplicationCommand, RunExecutionPort, RunExecutionUpdate, RunLaunchOutcome, RunLaunchRequest,
    StartedRun,
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
                "---\nschema_version: 1\nid: {id}\ntitle: Task {number}\nstatus: ready\ncontract_revision: 1\nacceptance_criteria:\n  - AC-{number:03}\ndependencies: []\nallowed_paths:\n  - crates/task_{number}/**\ntest_paths: []\nforbidden_paths: []\nrisk: low\n---\n\nImplement task {number}.\n"
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
}

struct FakeExecution {
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

#[test]
fn completion_releases_capacity_and_starts_the_next_queued_run() {
    let project = TempProject::with_tasks(4);
    let state = Arc::new(Mutex::new(FakeState::default()));
    let execution = FakeExecution {
        state: Arc::clone(&state),
    };
    let session = start_project(project.path(), "coordinator").unwrap();
    let mut coordinator = ProjectCoordinator::new(session, execution, ScheduleConfig::default());

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
}

#[test]
fn cancellation_stops_the_process_and_refills_capacity() {
    let project = TempProject::with_tasks(4);
    let state = Arc::new(Mutex::new(FakeState::default()));
    let execution = FakeExecution {
        state: Arc::clone(&state),
    };
    let session = start_project(project.path(), "coordinator").unwrap();
    let mut coordinator = ProjectCoordinator::new(session, execution, ScheduleConfig::default());
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
    let session = start_project(project.path(), "coordinator").unwrap();
    let mut coordinator = ProjectCoordinator::new(session, execution, ScheduleConfig::default());
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

fn context(command_id: &str) -> CommandContext {
    CommandContext {
        command_id: command_id.to_owned(),
        actor: "coordinator-test".to_owned(),
        occurred_at: "2026-08-01T12:00:00+09:00".to_owned(),
        base_commit: "a".repeat(40),
    }
}
