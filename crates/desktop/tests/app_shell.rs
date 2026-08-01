use gameforge_application::{
    AppShellContext, ConnectionState, DevelopmentBoardRecord, InboxItemRecord, compose_app_shell,
    compose_app_shell_with_inbox,
};
use gameforge_desktop::{
    ColorTheme, CommandResult, Route, TaskFilter, UiAction, UiState, execute_board_intent,
    reduce_ui_state, render_app, render_inbox,
};

fn view(
    connection: ConnectionState,
    rows: Vec<DevelopmentBoardRecord>,
) -> gameforge_application::AppShellView {
    compose_app_shell(
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
        },
        rows,
    )
}

fn row() -> DevelopmentBoardRecord {
    DevelopmentBoardRecord {
        task_id: "TASK-001".to_owned(),
        title: "砂の落下規則".to_owned(),
        task_status: "READY".to_owned(),
        current_run_id: Some("RUN-001".to_owned()),
        run_status: Some("AGENT_RUNNING".to_owned()),
        health_flags: Vec::new(),
    }
}

fn runnable_row() -> DevelopmentBoardRecord {
    DevelopmentBoardRecord {
        task_id: "TASK-READY".to_owned(),
        title: "砂の描画を追加".to_owned(),
        task_status: "READY".to_owned(),
        current_run_id: None,
        run_status: None,
        health_flags: Vec::new(),
    }
}

#[test]
fn renders_application_shell_and_development_board_from_view_dto() {
    let html = render_app(&view(ConnectionState::Connected, vec![row()]));

    assert!(html.contains("Powder Game"));
    assert!(html.contains("Development"));
    assert!(html.contains("TASK-001"));
    assert!(html.contains("砂の落下規則"));
    assert!(html.contains("AGENT_RUNNING"));
    assert!(html.contains("Inbox"));
    assert!(html.contains("接続中"));
    assert!(html.contains("brand-symbol"));
    assert!(html.contains("gameforge"));
    assert!(html.contains("BUILD CONTROL"));
    assert!(html.contains("aria-current=\"page\""));
    assert!(html.contains("Switch to night mode"));
    assert!(html.contains("PROJECTION · REV 42"));
    assert!(html.contains("Cancel Run"));
}

#[test]
fn emphasizes_the_next_runnable_task_and_exposes_task_inspection() {
    let html = render_app(&view(ConnectionState::Connected, vec![runnable_row()]));

    assert!(html.contains("NEXT ACTION"));
    assert!(html.contains("TASK-READY · 砂の描画を追加"));
    assert!(html.contains("Queue Task"));
    assert!(html.contains("Open task details"));
    assert!(html.contains("Select a task to inspect it"));
    assert!(html.contains("Filter by RUNNABLE"));
}

#[test]
fn renders_persistent_disconnect_banner_and_empty_state() {
    let html = render_app(&view(
        ConnectionState::Disconnected {
            reason: "Coordinator unavailable".to_owned(),
        },
        Vec::new(),
    ));

    assert!(html.contains("Coordinator unavailable"));
    assert!(html.contains("読み取り専用"));
    assert!(html.contains("実行対象のTaskはありません"));
}

#[test]
fn reducer_keeps_navigation_filter_and_selection_as_local_ui_state() {
    let state = UiState::default();
    let state = reduce_ui_state(state, &UiAction::Navigate(Route::Inbox));
    let state = reduce_ui_state(state, &UiAction::SetTaskFilter(TaskFilter::Running));
    let state = reduce_ui_state(state, &UiAction::SelectTask(Some("TASK-001".to_owned())));
    let state = reduce_ui_state(state, &UiAction::ToggleTheme);

    assert_eq!(state.route, Route::Inbox);
    assert_eq!(state.task_filter, TaskFilter::Running);
    assert_eq!(state.selected_task_id.as_deref(), Some("TASK-001"));
    assert_eq!(state.color_theme, ColorTheme::Night);
}

#[test]
fn queue_action_applies_the_single_scheduler_result_and_reports_preparing() {
    let current = view(ConnectionState::Connected, vec![runnable_row()]);
    let mut preparing = current.clone();
    preparing.projection_revision = 44;
    preparing.development.projection_revision = 44;
    preparing.development.summary.runnable = 0;
    preparing.development.summary.running = 1;
    preparing.development.task_rows[0].current_run_id = Some("RUN-TASK-READY-1".to_owned());
    preparing.development.task_rows[0].run_status = Some("PREPARING".to_owned());
    preparing.development.task_rows[0].can_queue = false;
    preparing.development.task_rows[0].queue_unavailable_reason =
        Some("このTaskには進行中または記録済みのRunがあります".to_owned());

    let effect = execute_board_intent(
        &current,
        gameforge_application::BoardIntent::QueueTask {
            task_id: "TASK-READY".to_owned(),
        },
        |command| {
            assert_eq!(
                command,
                gameforge_application::ApplicationCommand::QueueTaskRun {
                    task_id: "TASK-READY".to_owned(),
                    expected_projection_revision: 42,
                }
            );
            CommandResult::Applied(Box::new(preparing.clone()))
        },
    );

    assert_eq!(effect.updated_view, Some(preparing));
    assert!(effect.notice.contains("Queue登録完了"));
    assert!(effect.notice.contains("RUN-TASK-READY-1"));
    assert!(effect.notice.contains("PREPARING"));
}

#[test]
fn queue_action_keeps_the_current_view_when_the_coordinator_rejects_it() {
    let current = view(ConnectionState::Connected, vec![runnable_row()]);

    let effect = execute_board_intent(
        &current,
        gameforge_application::BoardIntent::QueueTask {
            task_id: "TASK-READY".to_owned(),
        },
        |_| CommandResult::Failed("projection revision conflict".to_owned()),
    );

    assert_eq!(effect.updated_view, None);
    assert!(effect.notice.contains("Queue登録失敗"));
    assert!(effect.notice.contains("projection revision conflict"));
}

#[test]
fn cancel_action_reports_the_cancelled_run_and_updates_the_view() {
    let current = view(ConnectionState::Connected, vec![row()]);
    let mut cancelled = view(
        ConnectionState::Connected,
        vec![DevelopmentBoardRecord {
            task_id: "TASK-001".to_owned(),
            title: "砂の落下規則".to_owned(),
            task_status: "READY".to_owned(),
            current_run_id: None,
            run_status: Some("CANCELLED".to_owned()),
            health_flags: Vec::new(),
        }],
    );
    cancelled.projection_revision = 43;
    cancelled.development.projection_revision = 43;

    let effect = execute_board_intent(
        &current,
        gameforge_application::BoardIntent::CancelRun {
            task_run_id: "RUN-001".to_owned(),
        },
        |command| {
            assert_eq!(
                command,
                gameforge_application::ApplicationCommand::CancelTaskRun {
                    task_run_id: "RUN-001".to_owned(),
                    expected_projection_revision: 42,
                }
            );
            CommandResult::Applied(Box::new(cancelled.clone()))
        },
    );

    assert_eq!(effect.updated_view, Some(cancelled));
    assert!(effect.notice.contains("Run取消し完了"));
    assert!(effect.notice.contains("RUN-001"));
    assert!(effect.notice.contains("CANCELLED"));
}

#[test]
fn renders_pending_input_requests_in_the_inbox() {
    let view = compose_app_shell_with_inbox(
        AppShellContext {
            project_name: "Powder Game".to_owned(),
            project_root: "/work/powder".to_owned(),
            main_commit: "91ad40c1".to_owned(),
            connection: ConnectionState::Connected,
            projection_revision: 5,
            last_synced_at: "2026-08-01T12:00:00+09:00".to_owned(),
            is_stale: false,
            inbox_count: 0,
            max_concurrent_task_runs: 1,
        },
        vec![DevelopmentBoardRecord {
            task_id: "TASK-001".to_owned(),
            title: "砂の落下規則".to_owned(),
            task_status: "READY".to_owned(),
            current_run_id: Some("RUN-TASK-001-1".to_owned()),
            run_status: Some("INPUT_REQUIRED".to_owned()),
            health_flags: Vec::new(),
        }],
        vec![InboxItemRecord {
            request_id: "INPUT-001".to_owned(),
            task_id: "TASK-001".to_owned(),
            task_run_id: "RUN-TASK-001-1".to_owned(),
            request_kind: "INPUT".to_owned(),
            prompt: "砂の優先方向を選んでください".to_owned(),
            status: "PENDING".to_owned(),
            requested_at: "2026-08-01T12:00:00+09:00".to_owned(),
        }],
    );

    let html = render_inbox(&view);

    assert!(html.contains("ACTION INBOX"));
    assert!(html.contains("INPUT-001"));
    assert!(html.contains("TASK-001"));
    assert!(html.contains("RUN-TASK-001-1"));
    assert!(html.contains("砂の優先方向を選んでください"));
    assert!(html.contains("PENDING"));
}
