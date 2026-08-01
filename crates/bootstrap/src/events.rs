use std::collections::BTreeMap;

use gameforge_application::RunLaunchRequest;
use gameforge_domain::TaskRun;
use gameforge_event_journal::{AggregateRef, EventEnvelope, EventHeader, SUPPORTED_SCHEMA_VERSION};

use crate::{BootstrapError, CommandContext};

pub(crate) fn queued_event(
    context: &CommandContext,
    task_run: &TaskRun,
    expected_projection_revision: u64,
) -> Result<EventEnvelope, BootstrapError> {
    let run_id = task_run.id().as_str();
    let mut payload = BTreeMap::new();
    payload.insert("task_id".to_owned(), task_run.task_id().as_str().to_owned());
    payload.insert("run_id".to_owned(), run_id.to_owned());
    payload.insert("state".to_owned(), "QUEUED".to_owned());
    payload.insert(
        "contract_revision".to_owned(),
        task_run.contract_revision().get().to_string(),
    );
    payload.insert(
        "base_commit".to_owned(),
        task_run.base_commit().as_str().to_owned(),
    );
    payload.insert(
        "expected_projection_revision".to_owned(),
        expected_projection_revision.to_string(),
    );
    let aggregate = AggregateRef::new("TaskRun", run_id)
        .map_err(|error| BootstrapError::Journal(error.to_string()))?;
    EventEnvelope::new(
        EventHeader {
            event_id: format!("EVT-{}", context.command_id),
            schema_version: SUPPORTED_SCHEMA_VERSION,
            occurred_at: context.occurred_at.clone(),
            aggregate,
            aggregate_version: 1,
            correlation_id: context.command_id.clone(),
            causation_id: None,
            actor: context.actor.clone(),
        },
        "TaskRunQueued",
        payload,
    )
    .map_err(|error| BootstrapError::Journal(error.to_string()))
}

pub(crate) fn cancelled_event(
    context: &CommandContext,
    run: &TaskRun,
    latest: &EventEnvelope,
    expected_projection_revision: u64,
) -> Result<EventEnvelope, BootstrapError> {
    let mut payload = BTreeMap::new();
    payload.insert(
        "expected_projection_revision".to_owned(),
        expected_projection_revision.to_string(),
    );
    task_run_state_event(context, run, latest, "CANCELLED", "CANCELLED", payload)
}

pub(crate) fn input_required_event(
    context: &CommandContext,
    run: &TaskRun,
    latest: &EventEnvelope,
    request_id: &str,
    prompt: &str,
    expected_projection_revision: u64,
) -> Result<EventEnvelope, BootstrapError> {
    let mut payload = BTreeMap::new();
    payload.insert("request_id".to_owned(), request_id.to_owned());
    payload.insert("request_prompt".to_owned(), prompt.to_owned());
    payload.insert(
        "expected_projection_revision".to_owned(),
        expected_projection_revision.to_string(),
    );
    task_run_state_event(
        context,
        run,
        latest,
        "INPUT_REQUIRED",
        "INPUT-REQUIRED",
        payload,
    )
}

pub(crate) fn input_answered_event(
    context: &CommandContext,
    run: &TaskRun,
    latest: &EventEnvelope,
    request_id: &str,
    answer: &str,
    expected_projection_revision: u64,
) -> Result<EventEnvelope, BootstrapError> {
    let mut payload = BTreeMap::new();
    payload.insert("resolved_request_id".to_owned(), request_id.to_owned());
    payload.insert("resolution_kind".to_owned(), "INPUT_ANSWERED".to_owned());
    payload.insert("input_answer".to_owned(), answer.to_owned());
    payload.insert(
        "expected_projection_revision".to_owned(),
        expected_projection_revision.to_string(),
    );
    task_run_state_event(
        context,
        run,
        latest,
        "AGENT_RUNNING",
        "INPUT-ANSWERED",
        payload,
    )
}

pub(crate) fn start_requested_event(
    context: &CommandContext,
    request: &RunLaunchRequest,
    preparation: &EventEnvelope,
) -> Result<EventEnvelope, BootstrapError> {
    let mut payload = BTreeMap::new();
    payload.insert("task_id".to_owned(), request.task_id.clone());
    payload.insert("run_id".to_owned(), request.task_run_id.clone());
    payload.insert(
        "contract_revision".to_owned(),
        request.contract_revision.to_string(),
    );
    payload.insert("base_commit".to_owned(), request.base_commit.clone());
    let aggregate = AggregateRef::new("Operation", format!("OP-{}-START", context.command_id))
        .map_err(|error| BootstrapError::Journal(error.to_string()))?;
    EventEnvelope::new(
        EventHeader {
            event_id: format!("EVT-{}-START-REQUESTED", context.command_id),
            schema_version: SUPPORTED_SCHEMA_VERSION,
            occurred_at: context.occurred_at.clone(),
            aggregate,
            aggregate_version: 1,
            correlation_id: context.command_id.clone(),
            causation_id: Some(preparation.header().event_id.clone()),
            actor: context.actor.clone(),
        },
        "TaskRunStartRequested",
        payload,
    )
    .map_err(|error| BootstrapError::Journal(error.to_string()))
}

pub(crate) fn supervisor_state_event(
    context: &CommandContext,
    request: &RunLaunchRequest,
    preparation: &EventEnvelope,
    requested: &EventEnvelope,
    state: &str,
    mut outcome_payload: BTreeMap<String, String>,
) -> Result<EventEnvelope, BootstrapError> {
    outcome_payload.insert("task_id".to_owned(), request.task_id.clone());
    outcome_payload.insert("run_id".to_owned(), request.task_run_id.clone());
    outcome_payload.insert("state".to_owned(), state.to_owned());
    let aggregate = AggregateRef::new("TaskRun", &request.task_run_id)
        .map_err(|error| BootstrapError::Journal(error.to_string()))?;
    EventEnvelope::new(
        EventHeader {
            event_id: format!("EVT-{}-{state}", context.command_id),
            schema_version: SUPPORTED_SCHEMA_VERSION,
            occurred_at: context.occurred_at.clone(),
            aggregate,
            aggregate_version: preparation.header().aggregate_version + 1,
            correlation_id: context.command_id.clone(),
            causation_id: Some(requested.header().event_id.clone()),
            actor: context.actor.clone(),
        },
        "TaskRunStateChanged",
        outcome_payload,
    )
    .map_err(|error| BootstrapError::Journal(error.to_string()))
}

pub(crate) fn execution_update_event(
    context: &CommandContext,
    run: &TaskRun,
    latest: &EventEnvelope,
    state: &str,
    payload: BTreeMap<String, String>,
) -> Result<EventEnvelope, BootstrapError> {
    task_run_state_event(context, run, latest, state, state, payload)
}

pub(crate) fn event_payload<'a>(
    event: &'a EventEnvelope,
    field: &'static str,
) -> Result<&'a str, BootstrapError> {
    event
        .payload()
        .get(field)
        .map(String::as_str)
        .ok_or_else(|| {
            BootstrapError::Journal(format!(
                "{} event is missing payload field {field}",
                event.event_type()
            ))
        })
}

fn task_run_state_event(
    context: &CommandContext,
    run: &TaskRun,
    latest: &EventEnvelope,
    state: &str,
    event_id_suffix: &str,
    mut payload: BTreeMap<String, String>,
) -> Result<EventEnvelope, BootstrapError> {
    payload.insert("task_id".to_owned(), run.task_id().as_str().to_owned());
    payload.insert("run_id".to_owned(), run.id().as_str().to_owned());
    payload.insert("state".to_owned(), state.to_owned());
    let aggregate = AggregateRef::new("TaskRun", run.id().as_str())
        .map_err(|error| BootstrapError::Journal(error.to_string()))?;
    EventEnvelope::new(
        EventHeader {
            event_id: format!("EVT-{}-{event_id_suffix}", context.command_id),
            schema_version: SUPPORTED_SCHEMA_VERSION,
            occurred_at: context.occurred_at.clone(),
            aggregate,
            aggregate_version: latest.header().aggregate_version + 1,
            correlation_id: context.command_id.clone(),
            causation_id: Some(latest.header().event_id.clone()),
            actor: context.actor.clone(),
        },
        "TaskRunStateChanged",
        payload,
    )
    .map_err(|error| BootstrapError::Journal(error.to_string()))
}
