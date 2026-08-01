use gameforgo_application::{
    AppShellContext, ConnectionState, DevelopmentBoardRecord, compose_app_shell,
};
use gameforgo_desktop::{Route, TaskFilter, UiAction, UiState, reduce_ui_state, render_app};

fn view(
    connection: ConnectionState,
    rows: Vec<DevelopmentBoardRecord>,
) -> gameforgo_application::AppShellView {
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

    assert_eq!(state.route, Route::Inbox);
    assert_eq!(state.task_filter, TaskFilter::Running);
    assert_eq!(state.selected_task_id.as_deref(), Some("TASK-001"));
}
