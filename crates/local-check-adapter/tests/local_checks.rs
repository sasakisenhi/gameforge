use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

use gameforge_application::{
    LocalVerificationPort, LocalVerificationRequest, LocalVerificationUpdate,
};
use gameforge_local_check_adapter::{LocalCheckCommand, LocalCheckRunner, LocalVerificationConfig};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

struct TempRepository {
    root: PathBuf,
    worktree: PathBuf,
    base_commit: String,
    head_commit: String,
}

impl TempRepository {
    fn with_committed_change() -> Self {
        let unique = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "gameforge-local-check-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(root.join("crates/task/src")).unwrap();
        fs::write(root.join(".gitignore"), ".game-dev/\n").unwrap();
        fs::write(
            root.join("crates/task/src/lib.rs"),
            "pub fn value() -> u8 { 1 }\n",
        )
        .unwrap();
        git(&root, &["init", "-b", "main"]);
        git(&root, &["add", "."]);
        commit(&root, "base");
        let base_commit = git(&root, &["rev-parse", "HEAD"]);

        let worktree = root.join(".game-dev/worktrees/run-task-001-1");
        fs::create_dir_all(worktree.parent().unwrap()).unwrap();
        let worktree_text = worktree.to_str().unwrap();
        git(
            &root,
            &[
                "worktree",
                "add",
                "-b",
                "gameforge/task/run-task-001-1",
                worktree_text,
                &base_commit,
            ],
        );
        fs::write(
            worktree.join("crates/task/src/lib.rs"),
            "pub fn value() -> u8 { 2 }\n",
        )
        .unwrap();
        git(&worktree, &["add", "."]);
        commit(&worktree, "change behavior");
        let head_commit = git(&worktree, &["rev-parse", "HEAD"]);
        Self {
            root,
            worktree,
            base_commit,
            head_commit,
        }
    }

    fn request(&self) -> LocalVerificationRequest {
        LocalVerificationRequest {
            task_run_id: "RUN-TASK-001-1".to_owned(),
            task_id: "TASK-001".to_owned(),
            base_commit: self.base_commit.clone(),
            worktree_path: self.worktree.display().to_string(),
        }
    }

    fn config(&self, commands: Vec<LocalCheckCommand>) -> LocalVerificationConfig {
        LocalVerificationConfig {
            project_root: self.root.clone(),
            commands,
        }
    }
}

impl Drop for TempRepository {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn reports_commit_changed_paths_and_completed_checks() {
    let repository = TempRepository::with_committed_change();
    let range = format!("{}..HEAD", repository.base_commit);
    let mut runner = LocalCheckRunner::new(repository.config(vec![LocalCheckCommand::new(
        "diff-check",
        "git",
        ["diff", "--check", &range],
        true,
    )]))
    .unwrap();

    runner.start_verification(&repository.request()).unwrap();
    let update = wait_for_update(&mut runner);

    assert_eq!(
        update,
        LocalVerificationUpdate::Passed {
            task_run_id: "RUN-TASK-001-1".to_owned(),
            head_commit: repository.head_commit.clone(),
            changed_paths: vec!["crates/task/src/lib.rs".to_owned()],
            completed_checks: vec!["diff-check".to_owned()],
            final_suite_passed: true,
        }
    );
    assert_eq!(runner.active_verification_count(), 0);
    let log = fs::read_to_string(
        repository
            .root
            .join(".game-dev/runtime/runs/run-task-001-1/local-checks.log"),
    )
    .unwrap();
    assert!(log.contains("== diff-check =="));
}

#[test]
fn returns_a_failed_update_for_a_nonzero_check() {
    let repository = TempRepository::with_committed_change();
    let mut runner = LocalCheckRunner::new(repository.config(vec![LocalCheckCommand::new(
        "must-have-no-diff",
        "git",
        [
            "diff",
            "--exit-code",
            repository.base_commit.as_str(),
            "HEAD",
        ],
        true,
    )]))
    .unwrap();

    runner.start_verification(&repository.request()).unwrap();
    let update = wait_for_update(&mut runner);

    let LocalVerificationUpdate::Failed {
        task_run_id,
        detail,
    } = update
    else {
        panic!("expected a failed Local Verification update");
    };
    assert_eq!(task_run_id, "RUN-TASK-001-1");
    assert!(detail.contains("must-have-no-diff failed with exit code 1"));
}

#[test]
fn rejects_an_uncommitted_worktree_before_running_checks() {
    let repository = TempRepository::with_committed_change();
    fs::write(
        repository.worktree.join("crates/task/src/lib.rs"),
        "pub fn value() -> u8 { 3 }\n",
    )
    .unwrap();
    let mut runner = LocalCheckRunner::new(repository.config(vec![LocalCheckCommand::new(
        "diff-check",
        "git",
        ["diff", "--check"],
        true,
    )]))
    .unwrap();

    runner.start_verification(&repository.request()).unwrap();
    let update = wait_for_update(&mut runner);

    let LocalVerificationUpdate::Failed { detail, .. } = update else {
        panic!("expected a failed Local Verification update");
    };
    assert!(detail.contains("requires a clean committed worktree"));
}

#[test]
fn cancellation_stops_a_running_check_without_emitting_failure() {
    let repository = TempRepository::with_committed_change();
    let mut runner = LocalCheckRunner::new(repository.config(vec![LocalCheckCommand::new(
        "slow-suite",
        "sleep",
        ["5"],
        true,
    )]))
    .unwrap();
    runner.start_verification(&repository.request()).unwrap();

    let started = Instant::now();
    runner.cancel_verification("RUN-TASK-001-1").unwrap();

    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(runner.active_verification_count(), 0);
    assert!(runner.poll_updates().is_empty());
}

fn wait_for_update(runner: &mut LocalCheckRunner) -> LocalVerificationUpdate {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(update) = runner.poll_updates().into_iter().next() {
            return update;
        }
        assert!(
            Instant::now() < deadline,
            "Local Verification did not finish"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn commit(repository: &Path, message: &str) {
    git(
        repository,
        &[
            "-c",
            "user.name=GameForge Test",
            "-c",
            "user.email=gameforge@example.invalid",
            "commit",
            "-m",
            message,
        ],
    );
}

fn git(repository: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}
