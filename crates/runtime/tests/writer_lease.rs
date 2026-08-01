use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
};

use gameforge_runtime::{CoordinatorError, ProjectWriterLease};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

struct TempProject(PathBuf);

impl TempProject {
    fn create() -> Self {
        let unique = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("gameforge-runtime-{}-{unique}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self(path)
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

#[test]
fn only_one_writer_lease_can_own_a_project() {
    let project = TempProject::create();
    let first = ProjectWriterLease::acquire(project.path(), "coordinator-1", 1).unwrap();

    assert!(matches!(
        ProjectWriterLease::acquire(project.path(), "coordinator-2", 1),
        Err(CoordinatorError::AlreadyOwned { .. })
    ));
    drop(first);

    let recovered = ProjectWriterLease::acquire(project.path(), "coordinator-3", 1).unwrap();
    assert_eq!(recovered.metadata().instance_id, "coordinator-3");
}

#[test]
fn a_separate_process_cannot_take_a_live_writer_lease() {
    let project = TempProject::create();
    let executable = std::env::current_exe().unwrap();
    let mut child = Command::new(executable)
        .args([
            "--ignored",
            "--exact",
            "holds_writer_lease_for_parent_test",
            "--nocapture",
        ])
        .env("GAMEFORGE_TEST_PROJECT", project.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    loop {
        line.clear();
        assert!(output.read_line(&mut line).unwrap() > 0);
        if line.contains("LEASE_ACQUIRED") {
            break;
        }
    }

    assert!(matches!(
        ProjectWriterLease::acquire(project.path(), "parent", 1),
        Err(CoordinatorError::AlreadyOwned { .. })
    ));

    child.stdin.take().unwrap().write_all(b"release\n").unwrap();
    assert!(child.wait().unwrap().success());
}

#[test]
#[ignore = "helper invoked by a parent process"]
fn holds_writer_lease_for_parent_test() {
    let Ok(project) = std::env::var("GAMEFORGE_TEST_PROJECT") else {
        return;
    };
    let _lease = ProjectWriterLease::acquire(project, "child", 1).unwrap();
    println!("LEASE_ACQUIRED");
    std::io::stdout().flush().unwrap();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).unwrap();
}
