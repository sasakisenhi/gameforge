use gameforge_application::{
    AppShellContext, ConnectionState, DevelopmentBoardRecord, compose_app_shell,
};
use gameforge_desktop::{
    ColorTheme, CommandResult, Route, TaskFilter, UiAction, UiState, execute_board_intent,
    reduce_ui_state, render_app,
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
fn queue_action_applies_the_coordinator_view_and_reports_completion() {
    let current = view(ConnectionState::Connected, vec![runnable_row()]);
    let mut queued = current.clone();
    queued.projection_revision = 43;
    queued.development.projection_revision = 43;
    queued.development.summary.runnable = 0;
    queued.development.summary.queued = 1;
    queued.development.task_rows[0].current_run_id = Some("RUN-TASK-READY-1".to_owned());
    queued.development.task_rows[0].run_status = Some("QUEUED".to_owned());
    queued.development.task_rows[0].can_queue = false;
    queued.development.task_rows[0].queue_unavailable_reason =
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
            CommandResult::Applied(Box::new(queued.clone()))
        },
    );

    assert_eq!(effect.updated_view, Some(queued));
    assert!(effect.notice.contains("Queue登録完了"));
    assert!(effect.notice.contains("RUN-TASK-READY-1"));
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
