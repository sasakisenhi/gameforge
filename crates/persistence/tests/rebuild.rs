use std::collections::BTreeMap;

use gameforge_event_journal::{AggregateRef, EventEnvelope, EventHeader};
use gameforge_persistence::{DevelopmentBoardRow, ProjectionError, ProjectionStore};
use gameforge_project_documents::load_task_document;

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

fn event(id: &str, version: u64, event_type: &str, state: Option<&str>) -> EventEnvelope {
    event_for_task(id, version, event_type, "TASK-001", state)
}

fn event_for_task(
    id: &str,
    version: u64,
    event_type: &str,
    task_id: &str,
    state: Option<&str>,
) -> EventEnvelope {
    let mut payload = BTreeMap::from([
        ("task_id".to_owned(), task_id.to_owned()),
        ("run_id".to_owned(), "RUN-001".to_owned()),
    ]);
    if let Some(state) = state {
        payload.insert("state".to_owned(), state.to_owned());
    }
    EventEnvelope::new(
        EventHeader {
            event_id: id.to_owned(),
            schema_version: 1,
            occurred_at: "2026-08-01T12:00:00+09:00".to_owned(),
            aggregate: AggregateRef::new("TaskRun", "RUN-001").unwrap(),
            aggregate_version: version,
            correlation_id: "CMD-1".to_owned(),
            causation_id: None,
            actor: "system:coordinator".to_owned(),
        },
        event_type,
        payload,
    )
    .unwrap()
}

#[test]
fn full_rebuild_matches_incremental_projection() {
    let task = load_task_document(TASK).unwrap();
    let events = vec![
        event("EVT-1", 1, "TaskRunQueued", Some("QUEUED")),
        event("EVT-2", 2, "TaskRunStateChanged", Some("AGENT_RUNNING")),
    ];

    let mut rebuilt = ProjectionStore::open_in_memory().unwrap();
    rebuilt
        .rebuild(std::slice::from_ref(&task), &events)
        .unwrap();

    let mut incremental = ProjectionStore::open_in_memory().unwrap();
    incremental
        .rebuild(std::slice::from_ref(&task), &[])
        .unwrap();
    for event in &events {
        incremental.apply_event(event).unwrap();
    }

    assert_eq!(
        rebuilt.development_board_rows().unwrap(),
        incremental.development_board_rows().unwrap()
    );
    assert_eq!(rebuilt.projection_revision().unwrap(), 2);
    assert_eq!(
        rebuilt.development_board_rows().unwrap(),
        vec![DevelopmentBoardRow {
            task_id: "TASK-001".to_owned(),
            title: "砂の落下規則".to_owned(),
            task_status: "READY".to_owned(),
            current_run_id: Some("RUN-001".to_owned()),
            run_status: Some("AGENT_RUNNING".to_owned()),
            health_flags: Vec::new(),
        }]
    );
}

#[test]
fn applying_the_same_event_is_idempotent() {
    let task = load_task_document(TASK).unwrap();
    let queued = event("EVT-1", 1, "TaskRunQueued", Some("QUEUED"));
    let mut store = ProjectionStore::open_in_memory().unwrap();
    store.rebuild(&[task], &[]).unwrap();

    store.apply_event(&queued).unwrap();
    store.apply_event(&queued).unwrap();

    assert_eq!(store.projection_revision().unwrap(), 1);
    assert_eq!(store.development_board_rows().unwrap().len(), 1);
}

#[test]
fn task_run_events_for_a_missing_task_are_rejected_without_being_applied() {
    for event_type in ["TaskRunQueued", "TaskRunStateChanged"] {
        let task = load_task_document(TASK).unwrap();
        let invalid = event_for_task("EVT-1", 1, event_type, "TASK-404", Some("QUEUED"));
        let corrected = event("EVT-1", 1, event_type, Some("QUEUED"));
        let mut store = ProjectionStore::open_in_memory().unwrap();
        store.rebuild(&[task], &[]).unwrap();

        assert_eq!(
            store.apply_event(&invalid),
            Err(ProjectionError::TaskNotFound {
                event_type: event_type.to_owned(),
                task_id: "TASK-404".to_owned(),
            })
        );
        assert_eq!(store.projection_revision().unwrap(), 0);
        assert_eq!(
            store.development_board_rows().unwrap()[0].current_run_id,
            None
        );

        store.apply_event(&corrected).unwrap();
        assert_eq!(store.projection_revision().unwrap(), 1);
    }
}
