use gameforge_application::{
    ActionError, AppShellContext, ApplicationCommand, BoardIntent, ConnectionState,
    DevelopmentBoardRecord, InboxIntent, InboxItemRecord, StartedRun, command_for_board_intent,
    command_for_inbox_intent, compose_app_shell, compose_app_shell_with_inbox,
};

fn context(connection: ConnectionState) -> AppShellContext {
    AppShellContext {
        project_name: "Powder Game".to_owned(),
        project_root: "/work/powder".to_owned(),
        main_commit: "91ad40c1".to_owned(),
        connection,
        projection_revision: 42,
        last_synced_at: "2026-08-01T12:00:00+09:00".to_owned(),
        is_stale: false,
        inbox_count: 2,
        max_concurrent_task_runs: 2,
    }
}

fn record(
    task_id: &str,
    task_status: &str,
    run_id: Option<&str>,
    run_status: Option<&str>,
) -> DevelopmentBoardRecord {
    DevelopmentBoardRecord {
        task_id: task_id.to_owned(),
        title: format!("{task_id} title"),
        task_status: task_status.to_owned(),
        current_run_id: run_id.map(str::to_owned),
        run_status: run_status.map(str::to_owned),
        health_flags: Vec::new(),
    }
}

#[test]
fn composes_summary_from_saved_projection_facts() {
    let view = compose_app_shell(
        context(ConnectionState::Connected),
        vec![
            record("TASK-1", "READY", Some("RUN-1"), Some("AGENT_RUNNING")),
            record("TASK-2", "READY", Some("RUN-2"), Some("QUEUED")),
            record("TASK-3", "WAITING_DEPENDENCY", None, None),
            record("TASK-4", "READY", Some("RUN-4"), Some("FAILED")),
        ],
    );

    assert_eq!(view.development.summary.running, 1);
    assert_eq!(view.development.summary.queued, 1);
    assert_eq!(view.development.summary.dependency_blocked, 1);
    assert_eq!(view.development.summary.failed, 1);
    assert_eq!(view.development.summary.needs_attention, 1);
    assert_eq!(view.development.task_rows.len(), 4);
    let run = &view.development.task_rows[0].artifacts;
    assert_eq!(
        run.worktree_path.as_deref(),
        Some("/work/powder/.game-dev/worktrees/RUN-1")
    );
    assert_eq!(
        run.runtime_log_path.as_deref(),
        Some("/work/powder/.game-dev/runtime/runs/RUN-1")
    );
    assert_eq!(
        run.diff_path.as_deref(),
        Some("/work/powder/.game-dev/runtime/runs/RUN-1/diff.patch")
    );
    assert!(view.development.task_rows[2].artifacts.worktree_path.is_none());
}

#[test]
fn maps_board_intent_to_a_versioned_application_command() {
    let view = compose_app_shell(
        context(ConnectionState::Connected),
        vec![record("TASK-1", "READY", None, None)],
    );
    let command = command_for_board_intent(
        &view,
        BoardIntent::QueueTask {
            task_id: "TASK-1".to_owned(),
        },
    )
    .unwrap();

    assert_eq!(
        command,
        ApplicationCommand::QueueTaskRun {
            task_id: "TASK-1".to_owned(),
            expected_projection_revision: 42,
        }
    );
}

#[test]
fn maps_cancel_intent_for_an_active_run() {
    let view = compose_app_shell(
        context(ConnectionState::Connected),
        vec![record("TASK-1", "READY", Some("RUN-1"), Some("PREPARING"))],
    );

    assert_eq!(
        command_for_board_intent(
            &view,
            BoardIntent::CancelRun {
                task_run_id: "RUN-1".to_owned(),
            },
        )
        .unwrap(),
        ApplicationCommand::CancelTaskRun {
            task_run_id: "RUN-1".to_owned(),
            expected_projection_revision: 42,
        }
    );
}

#[test]
fn rejects_mutation_from_disconnected_stale_or_busy_views() {
    let disconnected = compose_app_shell(
        context(ConnectionState::Disconnected {
            reason: "Coordinator unavailable".to_owned(),
        }),
        vec![record("TASK-1", "READY", None, None)],
    );
    assert_eq!(
        command_for_board_intent(
            &disconnected,
            BoardIntent::QueueTask {
                task_id: "TASK-1".to_owned(),
            },
        ),
        Err(ActionError::CoordinatorDisconnected)
    );

    let mut stale_context = context(ConnectionState::Connected);
    stale_context.is_stale = true;
    let stale = compose_app_shell(stale_context, vec![record("TASK-1", "READY", None, None)]);
    assert_eq!(
        command_for_board_intent(
            &stale,
            BoardIntent::QueueTask {
                task_id: "TASK-1".to_owned(),
            },
        ),
        Err(ActionError::StaleProjection)
    );

    let busy = compose_app_shell(
        context(ConnectionState::Connected),
        vec![record(
            "TASK-1",
            "ACTIVE",
            Some("RUN-1"),
            Some("AGENT_RUNNING"),
        )],
    );
    assert!(matches!(
        command_for_board_intent(
            &busy,
            BoardIntent::QueueTask {
                task_id: "TASK-1".to_owned(),
            },
        ),
        Err(ActionError::ActionUnavailable { .. })
    ));
}

#[test]
fn started_run_requires_every_external_handle() {
    assert!(StartedRun::new("resource-1", "worktree-1", "agent-1").is_ok());
    assert_eq!(
        StartedRun::new("", "worktree-1", "agent-1")
            .unwrap_err()
            .to_string(),
        "resource_lease_id must not be empty"
    );
    assert_eq!(
        StartedRun::new("resource-1", " ", "agent-1")
            .unwrap_err()
            .to_string(),
        "worktree_lease_id must not be empty"
    );
    assert_eq!(
        StartedRun::new("resource-1", "worktree-1", "")
            .unwrap_err()
            .to_string(),
        "agent_session_id must not be empty"
    );
}

#[test]
fn composes_pending_input_requests_for_the_inbox() {
    let view = compose_app_shell_with_inbox(
        context(ConnectionState::Connected),
        vec![record(
            "TASK-1",
            "READY",
            Some("RUN-1"),
            Some("INPUT_REQUIRED"),
        )],
        vec![InboxItemRecord {
            request_id: "INPUT-001".to_owned(),
            task_id: "TASK-1".to_owned(),
            task_run_id: "RUN-1".to_owned(),
            request_kind: "INPUT".to_owned(),
            prompt: "優先方向を選んでください".to_owned(),
            status: "PENDING".to_owned(),
            requested_at: "2026-08-01T12:00:00+09:00".to_owned(),
        }],
    );

    assert_eq!(view.inbox_count, 1);
    assert_eq!(view.inbox.pending, 1);
    assert_eq!(view.inbox.items.len(), 1);
    assert_eq!(view.inbox.items[0].request_id, "INPUT-001");
    assert_eq!(view.inbox.items[0].prompt, "優先方向を選んでください");

    assert_eq!(
        command_for_inbox_intent(
            &view,
            InboxIntent::AnswerInput {
                request_id: "INPUT-001".to_owned(),
                answer: "左方向を優先する".to_owned(),
            },
        )
        .unwrap(),
        ApplicationCommand::AnswerInputRequest {
            request_id: "INPUT-001".to_owned(),
            answer: "左方向を優先する".to_owned(),
            expected_projection_revision: 42,
        }
    );
    assert!(matches!(
        command_for_inbox_intent(
            &view,
            InboxIntent::AnswerInput {
                request_id: "INPUT-001".to_owned(),
                answer: " ".to_owned(),
            },
        ),
        Err(ActionError::ActionUnavailable { .. })
    ));
}
