use std::{
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdout, Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
};

use gameforge_runtime::{CoordinatorError, ProjectWriterLease, coordinator_status};

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
fn a_separate_process_is_detected_and_removes_metadata_on_normal_exit() {
    let project = TempProject::create();
    let (mut child, _output) = spawn_helper(
        project.path(),
        "holds_writer_lease_for_parent_test",
        "LEASE_ACQUIRED",
    );

    let metadata = coordinator_status(project.path())
        .unwrap()
        .expect("the locked project must report its coordinator metadata");
    assert_eq!(metadata.instance_id, "child");
    assert_eq!(metadata.process_id, child.id());
    assert_eq!(metadata.protocol_version, 1);
    assert_eq!(
        metadata.project_root,
        project.path().canonicalize().unwrap()
    );

    assert!(matches!(
        ProjectWriterLease::acquire(project.path(), "parent", 1),
        Err(CoordinatorError::AlreadyOwned { .. })
    ));

    child.stdin.take().unwrap().write_all(b"release\n").unwrap();
    assert!(child.wait().unwrap().success());
    assert_eq!(coordinator_status(project.path()).unwrap(), None);
    assert!(!metadata_path(project.path()).exists());
}

#[test]
fn stale_metadata_after_a_separate_process_crash_is_not_reported_as_running() {
    let project = TempProject::create();
    let (mut child, _output) = spawn_helper(
        project.path(),
        "holds_writer_lease_for_parent_test",
        "LEASE_ACQUIRED",
    );

    assert!(coordinator_status(project.path()).unwrap().is_some());
    child.kill().unwrap();
    let status = child.wait().unwrap();
    assert!(!status.success());
    assert!(metadata_path(project.path()).is_file());

    assert_eq!(coordinator_status(project.path()).unwrap(), None);
}

#[test]
fn a_locked_project_with_missing_metadata_is_an_explicit_error() {
    let project = TempProject::create();
    let (mut child, _output) = spawn_helper(
        project.path(),
        "holds_writer_lock_without_metadata_for_parent_test",
        "LOCK_ACQUIRED",
    );

    assert!(matches!(
        coordinator_status(project.path()),
        Err(CoordinatorError::MetadataMissing { .. })
    ));

    child.stdin.take().unwrap().write_all(b"release\n").unwrap();
    assert!(child.wait().unwrap().success());
}

#[test]
fn a_locked_project_with_corrupt_metadata_is_an_explicit_error() {
    let project = TempProject::create();
    fs::create_dir_all(project.path().join(".game-dev/runtime")).unwrap();
    fs::write(metadata_path(project.path()), "this is not metadata").unwrap();
    let (mut child, _output) = spawn_helper(
        project.path(),
        "holds_writer_lock_without_metadata_for_parent_test",
        "LOCK_ACQUIRED",
    );

    assert!(matches!(
        coordinator_status(project.path()),
        Err(CoordinatorError::MetadataInvalid { .. })
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

#[test]
#[ignore = "helper invoked by a parent process"]
fn holds_writer_lock_without_metadata_for_parent_test() {
    let Ok(project) = std::env::var("GAMEFORGE_TEST_PROJECT") else {
        return;
    };
    let runtime_directory = Path::new(&project).join(".game-dev/runtime");
    fs::create_dir_all(&runtime_directory).unwrap();
    let lock_file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(runtime_directory.join("writer.lock"))
        .unwrap();
    lock_file.try_lock().unwrap();
    println!("LOCK_ACQUIRED");
    std::io::stdout().flush().unwrap();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).unwrap();
}

fn spawn_helper(
    project_root: &Path,
    test_name: &str,
    ready_marker: &str,
) -> (Child, BufReader<ChildStdout>) {
    let executable = std::env::current_exe().unwrap();
    let mut child = Command::new(executable)
        .args(["--ignored", "--exact", test_name, "--nocapture"])
        .env("GAMEFORGE_TEST_PROJECT", project_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    loop {
        line.clear();
        assert!(output.read_line(&mut line).unwrap() > 0);
        if line.contains(ready_marker) {
            break;
        }
    }
    (child, output)
}

fn metadata_path(project_root: &Path) -> PathBuf {
    project_root.join(".game-dev/runtime/coordinator.meta")
}
