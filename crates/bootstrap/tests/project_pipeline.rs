use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use gameforgo_bootstrap::{rebuild_project, validate_project};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

struct TempProject(PathBuf);

impl TempProject {
    fn create() -> Self {
        let unique = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "gameforgo-bootstrap-{}-{unique}",
            std::process::id()
        ));
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
