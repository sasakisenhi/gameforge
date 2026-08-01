use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use gameforgo_cli::{CliError, run};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

struct TempProject(PathBuf);

impl TempProject {
    fn create() -> Self {
        let unique = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("gameforgo-cli-{}-{unique}", std::process::id()));
        let tasks = root.join(".game-dev/tasks");
        fs::create_dir_all(&tasks).unwrap();
        fs::write(tasks.join("TASK-001.md"), TASK).unwrap();
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

const TASK: &str = r"---
schema_version: 1
id: TASK-001
title: 砂の落下規則
status: ready
contract_revision: 1
acceptance_criteria: [AC-001]
dependencies: []
allowed_paths: [crates/game_logic/src/sand/**]
test_paths: [crates/game_logic/tests/sand/**]
forbidden_paths: [crates/game_runtime/**]
risk: low
---

# 目的
砂を落下させる。
";

#[test]
fn validate_and_rebuild_commands_report_human_readable_results() {
    let project = TempProject::create();
    let root = project.path().display().to_string();

    let mut validation_output = Vec::new();
    run(["validate", root.as_str()], &mut validation_output).unwrap();
    let validation_output = String::from_utf8(validation_output).unwrap();
    assert!(validation_output.contains("検証成功: 1件のTask"));
    assert!(validation_output.contains("TASK-001"));

    let mut rebuild_output = Vec::new();
    run(["rebuild", root.as_str()], &mut rebuild_output).unwrap();
    let rebuild_output = String::from_utf8(rebuild_output).unwrap();
    assert!(rebuild_output.contains("Read Model再構築完了"));
    assert!(rebuild_output.contains("TASK-001 | READY"));
}

#[test]
fn missing_command_returns_usage_error() {
    let mut output = Vec::new();
    assert!(matches!(
        run(std::iter::empty::<&str>(), &mut output),
        Err(CliError::Usage(_))
    ));
}
