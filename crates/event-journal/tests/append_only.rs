use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

use gameforge_event_journal::{
    AggregateRef, EventEnvelope, EventHeader, EventJournal, JournalError,
};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

fn temp_journal() -> PathBuf {
    let unique = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "gameforge-journal-{}-{unique}.jsonl",
        std::process::id()
    ))
}

fn event(id: &str, version: u64, event_type: &str) -> EventEnvelope {
    EventEnvelope::new(
        EventHeader {
            event_id: id.to_owned(),
            schema_version: 1,
            occurred_at: "2026-08-01T12:00:00+09:00".to_owned(),
            aggregate: AggregateRef::new("TaskRun", "RUN-1").unwrap(),
            aggregate_version: version,
            correlation_id: "CMD-1".to_owned(),
            causation_id: None,
            actor: "system:coordinator".to_owned(),
        },
        event_type,
        BTreeMap::from([
            ("task_id".to_owned(), "TASK-1".to_owned()),
            ("run_id".to_owned(), "RUN-1".to_owned()),
        ]),
    )
    .unwrap()
}

#[test]
fn appended_events_can_be_reopened_without_losing_order() {
    let path = temp_journal();
    let mut journal = EventJournal::open(&path).unwrap();
    journal.append(&event("EVT-1", 1, "TaskRunQueued")).unwrap();
    journal
        .append(&event("EVT-2", 2, "TaskRunPreparationStarted"))
        .unwrap();
    drop(journal);

    let reopened = EventJournal::open(&path).unwrap();
    let events = reopened.events();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].header().event_id, "EVT-1");
    assert_eq!(events[1].header().event_id, "EVT-2");

    fs::remove_file(path).unwrap();
}

#[test]
fn rejects_duplicate_ids_and_aggregate_version_gaps_before_writing() {
    let path = temp_journal();
    let mut journal = EventJournal::open(&path).unwrap();
    journal.append(&event("EVT-1", 1, "TaskRunQueued")).unwrap();

    assert!(matches!(
        journal.append(&event("EVT-1", 2, "TaskRunPreparationStarted")),
        Err(JournalError::DuplicateEventId(_))
    ));
    assert!(matches!(
        journal.append(&event("EVT-3", 3, "TaskRunPreparationStarted")),
        Err(JournalError::AggregateVersion { .. })
    ));
    assert_eq!(EventJournal::open(&path).unwrap().events().len(), 1);

    fs::remove_file(path).unwrap();
}

#[test]
fn rejects_corrupted_journal_instead_of_ignoring_it() {
    let path = temp_journal();
    fs::write(&path, "{not-json}\n").unwrap();
    assert!(matches!(
        EventJournal::open(&path),
        Err(JournalError::InvalidRecord { line: 1, .. })
    ));
    fs::remove_file(path).unwrap();
}
