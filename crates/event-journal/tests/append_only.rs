use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
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

fn append_bytes(path: &PathBuf, bytes: &[u8]) {
    let mut file = OpenOptions::new().append(true).open(path).unwrap();
    file.write_all(bytes).unwrap();
    file.sync_data().unwrap();
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

#[test]
fn identifies_an_unterminated_incomplete_final_record() {
    let path = temp_journal();
    let mut journal = EventJournal::open(&path).unwrap();
    journal.append(&event("EVT-1", 1, "TaskRunQueued")).unwrap();
    drop(journal);

    let record = serde_json::to_vec(&event("EVT-2", 2, "TaskRunPreparationStarted")).unwrap();
    let incomplete = &record[..record.len() - 1];
    append_bytes(&path, incomplete);

    assert!(matches!(
        EventJournal::open(&path),
        Err(JournalError::IncompleteTail {
            line: 2,
            incomplete_bytes,
        }) if incomplete_bytes == incomplete.len() as u64
    ));

    fs::remove_file(path).unwrap();
}

#[test]
fn recovers_only_the_incomplete_tail_and_continues_aggregate_versions() {
    let path = temp_journal();
    let mut journal = EventJournal::open(&path).unwrap();
    journal.append(&event("EVT-1", 1, "TaskRunQueued")).unwrap();
    journal
        .append(&event("EVT-2", 2, "TaskRunPreparationStarted"))
        .unwrap();
    drop(journal);

    let retained_bytes = fs::metadata(&path).unwrap().len();
    let record = serde_json::to_vec(&event("EVT-3", 3, "TaskRunStarted")).unwrap();
    let incomplete = &record[..record.len() - 1];
    append_bytes(&path, incomplete);

    let recovery = EventJournal::recover_incomplete_tail(&path).unwrap();
    assert_eq!(recovery.truncated_bytes(), incomplete.len() as u64);
    assert_eq!(recovery.retained_bytes(), retained_bytes);
    assert_eq!(recovery.recovered_line(), 3);
    assert_eq!(fs::metadata(&path).unwrap().len(), retained_bytes);

    let mut recovered = EventJournal::open(&path).unwrap();
    recovered
        .append(&event("EVT-3", 3, "TaskRunStarted"))
        .unwrap();
    assert_eq!(EventJournal::open(&path).unwrap().events().len(), 3);

    fs::remove_file(path).unwrap();
}

#[test]
fn refuses_to_recover_a_newline_terminated_corrupt_record() {
    let path = temp_journal();
    fs::write(&path, "{not-json}\n").unwrap();
    let original = fs::read(&path).unwrap();

    assert!(matches!(
        EventJournal::recover_incomplete_tail(&path),
        Err(JournalError::InvalidRecord { line: 1, .. })
    ));
    assert_eq!(fs::read(&path).unwrap(), original);

    fs::remove_file(path).unwrap();
}

#[test]
fn refuses_to_recover_when_an_earlier_record_is_corrupt() {
    let path = temp_journal();
    let mut journal = EventJournal::open(&path).unwrap();
    journal.append(&event("EVT-1", 1, "TaskRunQueued")).unwrap();
    drop(journal);

    append_bytes(&path, b"{not-json}\n");
    let record = serde_json::to_vec(&event("EVT-2", 2, "TaskRunPreparationStarted")).unwrap();
    append_bytes(&path, &record[..record.len() - 1]);
    let original = fs::read(&path).unwrap();

    assert!(matches!(
        EventJournal::recover_incomplete_tail(&path),
        Err(JournalError::InvalidRecord { line: 2, .. })
    ));
    assert_eq!(fs::read(&path).unwrap(), original);

    fs::remove_file(path).unwrap();
}

#[test]
fn refuses_to_recover_an_unterminated_syntax_error() {
    let path = temp_journal();
    fs::write(&path, "{not-json}").unwrap();
    let original = fs::read(&path).unwrap();

    assert!(matches!(
        EventJournal::recover_incomplete_tail(&path),
        Err(JournalError::InvalidRecord { line: 1, .. })
    ));
    assert_eq!(fs::read(&path).unwrap(), original);

    fs::remove_file(path).unwrap();
}

#[test]
fn preserves_a_complete_final_record_without_a_newline() {
    let path = temp_journal();
    let record = serde_json::to_vec(&event("EVT-1", 1, "TaskRunQueued")).unwrap();
    fs::write(&path, &record).unwrap();

    assert_eq!(EventJournal::open(&path).unwrap().events().len(), 1);
    assert!(matches!(
        EventJournal::recover_incomplete_tail(&path),
        Err(JournalError::NoIncompleteTail)
    ));
    assert_eq!(fs::read(&path).unwrap(), record);

    fs::remove_file(path).unwrap();
}

#[test]
fn recovers_a_record_cut_in_the_middle_of_a_utf8_character() {
    let path = temp_journal();
    let unicode_event = EventEnvelope::new(
        EventHeader {
            event_id: "EVT-1".to_owned(),
            schema_version: 1,
            occurred_at: "2026-08-01T12:00:00+09:00".to_owned(),
            aggregate: AggregateRef::new("TaskRun", "RUN-1").unwrap(),
            aggregate_version: 1,
            correlation_id: "CMD-1".to_owned(),
            causation_id: None,
            actor: "system:調整役".to_owned(),
        },
        "TaskRunQueued",
        BTreeMap::new(),
    )
    .unwrap();
    let record = serde_json::to_vec(&unicode_event).unwrap();
    let unicode_start = record
        .windows("調".len())
        .position(|window| window == "調".as_bytes())
        .unwrap();
    let incomplete = &record[..=unicode_start];
    fs::write(&path, incomplete).unwrap();

    assert!(matches!(
        EventJournal::open(&path),
        Err(JournalError::IncompleteTail { line: 1, .. })
    ));
    let recovery = EventJournal::recover_incomplete_tail(&path).unwrap();
    assert_eq!(recovery.truncated_bytes(), incomplete.len() as u64);
    assert_eq!(recovery.retained_bytes(), 0);
    assert!(fs::read(&path).unwrap().is_empty());

    fs::remove_file(path).unwrap();
}
