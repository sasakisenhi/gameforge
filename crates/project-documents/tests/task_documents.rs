use gameforge_project_documents::{
    ChangedPathViolation, DocumentError, load_task_document, validate_changed_paths,
    validate_task_documents,
};

fn task_document(id: &str, dependency: &str) -> String {
    format!(
        r"---
schema_version: 1
id: {id}
title: 砂の落下規則
status: ready
contract_revision: 1
acceptance_criteria:
  - AC-001
dependencies:{dependency}
allowed_paths:
  - crates/game_logic/src/sand/**
test_paths:
  - crates/game_logic/tests/sand/**
forbidden_paths:
  - crates/game_runtime/**
risk: low
---

# 目的

空きセルが下にある場合、砂を下方向へ移動させる。
"
    )
}

#[test]
fn loads_typed_front_matter_and_preserves_markdown_body() {
    let source = task_document("TASK-001", " []");
    let task = load_task_document(&source).unwrap();

    assert_eq!(task.id().as_str(), "TASK-001");
    assert_eq!(task.title(), "砂の落下規則");
    assert_eq!(task.contract_revision().get(), 1);
    assert_eq!(task.allowed_paths(), ["crates/game_logic/src/sand/**"]);
    assert!(task.body().contains("# 目的"));
    assert_eq!(task.content_hash().as_str().len(), 64);
}

#[test]
fn rejects_unknown_schema_and_unsafe_paths() {
    let unsupported =
        task_document("TASK-001", " []").replacen("schema_version: 1", "schema_version: 2", 1);
    assert!(matches!(
        load_task_document(&unsupported),
        Err(DocumentError::UnsupportedSchemaVersion(2))
    ));

    let unsafe_path = task_document("TASK-001", " []").replacen(
        "crates/game_logic/src/sand/**",
        "../outside/**",
        1,
    );
    assert!(matches!(
        load_task_document(&unsafe_path),
        Err(DocumentError::UnsafePath(_))
    ));
}

#[test]
fn validates_dependencies_across_loaded_documents() {
    let one = load_task_document(&task_document(
        "TASK-001",
        "\n  - task_id: TASK-002\n    kind: blocks_start\n    reason: APIが必要",
    ))
    .unwrap();
    let two = load_task_document(&task_document(
        "TASK-002",
        "\n  - task_id: TASK-001\n    kind: blocks_integration\n    reason: 設定が必要",
    ))
    .unwrap();

    assert!(matches!(
        validate_task_documents(&[one, two]),
        Err(DocumentError::InvalidTaskGraph(_))
    ));
}

#[test]
fn rejects_unknown_front_matter_fields() {
    let source =
        task_document("TASK-001", " []").replacen("risk: low", "risk: low\nunexpected: value", 1);
    assert!(matches!(
        load_task_document(&source),
        Err(DocumentError::InvalidFrontMatter(_))
    ));
}

#[test]
fn validates_changed_path_against_exact_pattern() {
    let source = task_document("TASK-001", " []").replacen(
        "crates/game_logic/src/sand/**",
        "crates/game_logic/Cargo.toml",
        1,
    );
    let task = load_task_document(&source).unwrap();

    validate_changed_paths(&task, ["crates/game_logic/Cargo.toml"]).unwrap();

    let error = validate_changed_paths(&task, ["crates/game_logic/src/lib.rs"]).unwrap_err();
    assert_eq!(
        error.violations(),
        [ChangedPathViolation::OutsideAllowedAndTestPaths {
            path: "crates/game_logic/src/lib.rs".to_owned(),
        }]
    );
}

#[test]
fn single_star_matches_exactly_one_path_segment() {
    let source = task_document("TASK-001", " []").replacen(
        "crates/game_logic/src/sand/**",
        "crates/*/Cargo.toml",
        1,
    );
    let task = load_task_document(&source).unwrap();

    validate_changed_paths(&task, ["crates/game_logic/Cargo.toml"]).unwrap();

    let error = validate_changed_paths(&task, ["crates/game_logic/src/Cargo.toml"]).unwrap_err();
    assert!(matches!(
        error.violations(),
        [ChangedPathViolation::OutsideAllowedAndTestPaths { .. }]
    ));
}

#[test]
fn double_star_matches_path_segments_recursively() {
    let source = task_document("TASK-001", " []").replacen(
        "crates/game_logic/src/sand/**",
        "crates/**/sand/*",
        1,
    );
    let task = load_task_document(&source).unwrap();

    validate_changed_paths(
        &task,
        [
            "crates/sand/rules.rs",
            "crates/game_logic/src/sand/rules.rs",
        ],
    )
    .unwrap();
}

#[test]
fn accepts_changed_paths_matched_by_test_paths() {
    let task = load_task_document(&task_document("TASK-001", " []")).unwrap();

    validate_changed_paths(&task, ["crates/game_logic/tests/sand/falls.rs"]).unwrap();
}

#[test]
fn forbidden_patterns_take_priority_over_allowed_patterns() {
    let source = task_document("TASK-001", " []")
        .replacen("crates/game_logic/src/sand/**", "crates/**", 1)
        .replacen(
            "  - crates/game_runtime/**",
            "  - crates/game_runtime/**\n  - crates/*/src/**",
            1,
        );
    let task = load_task_document(&source).unwrap();

    let error = validate_changed_paths(&task, ["crates/game_runtime/src/lib.rs"]).unwrap_err();

    assert_eq!(
        error.violations(),
        [ChangedPathViolation::ForbiddenPath {
            path: "crates/game_runtime/src/lib.rs".to_owned(),
            pattern: "crates/*/src/**".to_owned(),
        }]
    );
}

#[test]
fn reports_every_violation_in_deterministic_path_order() {
    let task = load_task_document(&task_document("TASK-001", " []")).unwrap();

    let error = validate_changed_paths(
        &task,
        [
            "outside/z.rs",
            "crates/game_logic/tests/sand/falls.rs",
            "crates/game_runtime/src/lib.rs",
            "/absolute.rs",
            "another/file.rs",
            "../escape.rs",
            "crates/game_logic/src/sand/rules.rs",
        ],
    )
    .unwrap_err();

    assert_eq!(
        error.into_violations(),
        [
            ChangedPathViolation::InvalidRelativePath {
                path: "../escape.rs".to_owned(),
            },
            ChangedPathViolation::InvalidRelativePath {
                path: "/absolute.rs".to_owned(),
            },
            ChangedPathViolation::OutsideAllowedAndTestPaths {
                path: "another/file.rs".to_owned(),
            },
            ChangedPathViolation::ForbiddenPath {
                path: "crates/game_runtime/src/lib.rs".to_owned(),
                pattern: "crates/game_runtime/**".to_owned(),
            },
            ChangedPathViolation::OutsideAllowedAndTestPaths {
                path: "outside/z.rs".to_owned(),
            },
        ]
    );
}
