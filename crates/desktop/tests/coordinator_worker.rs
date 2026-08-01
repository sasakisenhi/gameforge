use std::{
    collections::VecDeque,
    fs,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use gameforge_application::{
    ApplicationCommand, RunExecutionPort, RunExecutionUpdate, RunLaunchOutcome, RunLaunchRequest,
    StartedRun,
};
use gameforge_bootstrap::{ProjectCoordinator, start_project};
use gameforge_desktop::{CoordinatorWorkerContext, spawn_coordinator_worker};
use gameforge_runtime::ScheduleConfig;

static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

struct TempProject(PathBuf);

impl TempProject {
    fn create() -> Self {
        let unique = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "gameforge-desktop-worker-{}-{unique}",
            std::process::id()
        ));
        let tasks = root.join(".game-dev/tasks");
        fs::create_dir_all(&tasks).unwrap();
        fs::write(
            tasks.join("TASK-001.md"),
            "---\nschema_version: 1\nid: TASK-001\ntitle: Task 1\nstatus: ready\ncontract_revision: 1\nacceptance_criteria:\n  - AC-001\ndependencies: []\nallowed_paths:\n  - crates/task_1/**\ntest_paths: []\nforbidden_paths: []\nrisk: low\n---\n\nImplement task 1.\n",
        )
        .unwrap();
        Self(root)
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[derive(Default)]
struct FakeState {
    updates: VecDeque<RunExecutionUpdate>,
}

struct FakeExecution(Arc<Mutex<FakeState>>);

impl RunExecutionPort for FakeExecution {
    fn start_run(&mut self, request: &RunLaunchRequest) -> RunLaunchOutcome {
        RunLaunchOutcome::Started(
            StartedRun::new(
                "resource-1",
                "worktree-1",
                format!("agent-{}", request.task_run_id),
            )
            .unwrap(),
        )
    }

    fn poll_updates(&mut self) -> Vec<RunExecutionUpdate> {
        self.0.lock().unwrap().updates.drain(..).collect()
    }
}

#[test]
fn worker_polls_execution_updates_without_an_additional_user_command() {
    let project = TempProject::create();
    let state = Arc::new(Mutex::new(FakeState::default()));
    let session = start_project(&project.0, "worker-test").unwrap();
    let coordinator = ProjectCoordinator::new(
        session,
        FakeExecution(Arc::clone(&state)),
        ScheduleConfig::default(),
    );
    let worker = spawn_coordinator_worker(
        coordinator,
        CoordinatorWorkerContext {
            actor: "desktop-worker-test".to_owned(),
            base_commit: "a".repeat(40),
        },
        Duration::from_millis(10),
    )
    .unwrap();

    worker
        .execute(ApplicationCommand::QueueTaskRun {
            task_id: "TASK-001".to_owned(),
            expected_projection_revision: 0,
        })
        .unwrap();
    state
        .lock()
        .unwrap()
        .updates
        .push_back(RunExecutionUpdate::Completed {
            task_run_id: "RUN-TASK-001-1".to_owned(),
            agent_session_id: "agent-RUN-TASK-001-1".to_owned(),
        });

    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let snapshot = worker.snapshot().unwrap();
        if snapshot.development_board[0].run_status.as_deref() == Some("LOCAL_CHECKING") {
            break;
        }
        assert!(Instant::now() < deadline, "worker did not poll completion");
        thread::sleep(Duration::from_millis(10));
    }
}
