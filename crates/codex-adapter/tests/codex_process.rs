#![cfg(unix)]

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

use gameforge_application::{
    RunExecutionPort, RunExecutionUpdate, RunLaunchOutcome, RunLaunchRequest,
};
use gameforge_codex_adapter::{CodexRunExecution, CodexRunExecutionConfig};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

struct TempRepo(PathBuf);

impl TempRepo {
    fn create() -> Self {
        let unique = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "gameforge-codex-adapter-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(root.join(".game-dev/tasks")).unwrap();
        fs::write(root.join(".gitignore"), ".game-dev/\n").unwrap();
        fs::write(
            root.join(".game-dev/tasks/TASK-001.md"),
            "---\nid: TASK-001\n---\n\nImplement the task.\n",
        )
        .unwrap();
        run_git(&root, &["init", "-q"]);
        run_git(&root, &["config", "user.name", "GameForge Test"]);
        run_git(
            &root,
            &["config", "user.email", "gameforge@example.invalid"],
        );
        run_git(&root, &["add", ".gitignore"]);
        run_git(&root, &["commit", "-qm", "initial"]);
        Self(root)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn head(&self) -> String {
        String::from_utf8(
            Command::new("git")
                .arg("-C")
                .arg(&self.0)
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap()
                .stdout,
        )
        .unwrap()
        .trim()
        .to_owned()
    }
}

impl Drop for TempRepo {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn creates_a_git_worktree_and_starts_a_codex_app_server_turn() {
    let repo = TempRepo::create();
    let protocol_log = repo.path().join("protocol-input.jsonl");
    let fake_codex = repo.path().join("fake-codex");
    fs::write(
        &fake_codex,
        format!(
            r#"#!/bin/sh
read initialize
printf '%s\n' "$initialize" >> '{}'
printf '%s\n' '{{"id":1,"result":{{"codexHome":"/tmp","platformFamily":"unix","platformOs":"linux","userAgent":"fake"}}}}'
read initialized
printf '%s\n' "$initialized" >> '{}'
read thread_start
printf '%s\n' "$thread_start" >> '{}'
printf '%s\n' '{{"id":2,"result":{{"thread":{{"id":"thread-1"}}}}}}'
read turn_start
printf '%s\n' "$turn_start" >> '{}'
printf '%s\n' '{{"id":3,"result":{{"turn":{{"id":"turn-1"}}}}}}'
printf '%s\n' '{{"method":"turn/completed","params":{{"threadId":"thread-1","turn":{{"id":"turn-1","items":[],"status":"completed"}}}}}}'
while read ignored; do :; done
"#,
            protocol_log.display(),
            protocol_log.display(),
            protocol_log.display(),
            protocol_log.display(),
        ),
    )
    .unwrap();
    let mut permissions = fs::metadata(&fake_codex).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fake_codex, permissions).unwrap();

    let mut adapter = CodexRunExecution::new(CodexRunExecutionConfig {
        project_root: repo.path().to_path_buf(),
        codex_executable: fake_codex,
        max_concurrent_task_runs: 3,
        startup_timeout: Duration::from_secs(2),
    })
    .unwrap();
    let outcome = adapter.start_run(&RunLaunchRequest {
        task_run_id: "RUN-TASK-001-1".to_owned(),
        task_id: "TASK-001".to_owned(),
        contract_revision: 1,
        base_commit: repo.head(),
    });

    let RunLaunchOutcome::Started(started) = outcome else {
        panic!("the real adapter must start the run");
    };
    assert_eq!(started.agent_session_id(), "thread-1");
    assert!(
        repo.path()
            .join(".game-dev/worktrees/run-task-001-1/.git")
            .exists()
    );
    assert_eq!(adapter.active_run_count(), 1);

    let deadline = Instant::now() + Duration::from_secs(2);
    let update = loop {
        if let Some(update) = adapter.poll_updates().into_iter().next() {
            break update;
        }
        assert!(
            Instant::now() < deadline,
            "Codex completion was not observed"
        );
        thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(
        update,
        RunExecutionUpdate::Completed {
            task_run_id: "RUN-TASK-001-1".to_owned(),
            agent_session_id: "thread-1".to_owned(),
            red_evidence_present: false,
            green_evidence_present: false,
        }
    );
    assert_eq!(adapter.active_run_count(), 0);

    let protocol = fs::read_to_string(protocol_log).unwrap();
    assert!(protocol.contains("\"method\":\"initialize\""));
    assert!(protocol.contains("\"method\":\"thread/start\""));
    assert!(protocol.contains("\"method\":\"turn/start\""));
    assert!(protocol.contains("Implement the task."));
}

#[test]
fn cancellation_stops_the_codex_process_and_releases_its_slot() {
    let repo = TempRepo::create();
    let fake_codex = repo.path().join("fake-codex");
    fs::write(
        &fake_codex,
        r#"#!/bin/sh
read initialize
printf '%s\n' '{"id":1,"result":{"codexHome":"/tmp","platformFamily":"unix","platformOs":"linux","userAgent":"fake"}}'
read initialized
read thread_start
printf '%s\n' '{"id":2,"result":{"thread":{"id":"thread-1"}}}'
read turn_start
printf '%s\n' '{"id":3,"result":{"turn":{"id":"turn-1"}}}'
while read ignored; do :; done
"#,
    )
    .unwrap();
    let mut permissions = fs::metadata(&fake_codex).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&fake_codex, permissions).unwrap();

    let mut adapter = CodexRunExecution::new(CodexRunExecutionConfig {
        project_root: repo.path().to_path_buf(),
        codex_executable: fake_codex,
        max_concurrent_task_runs: 3,
        startup_timeout: Duration::from_secs(2),
    })
    .unwrap();
    let outcome = adapter.start_run(&RunLaunchRequest {
        task_run_id: "RUN-TASK-001-1".to_owned(),
        task_id: "TASK-001".to_owned(),
        contract_revision: 1,
        base_commit: repo.head(),
    });

    assert!(matches!(outcome, RunLaunchOutcome::Started(_)));
    assert_eq!(adapter.active_run_count(), 1);
    adapter.cancel_run("RUN-TASK-001-1").unwrap();
    assert_eq!(adapter.active_run_count(), 0);
}

fn run_git(root: &Path, arguments: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(arguments)
        .status()
        .unwrap();
    assert!(status.success(), "git {arguments:?} failed");
}
