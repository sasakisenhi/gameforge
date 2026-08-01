use std::{path::PathBuf, sync::OnceLock};

use dioxus::prelude::*;
use gameforgo_application::{AppShellContext, AppShellView, ConnectionState, compose_app_shell};
use gameforgo_bootstrap::{BootstrapError, start_project};
use gameforgo_desktop::App;

static INITIAL_VIEW: OnceLock<AppShellView> = OnceLock::new();

fn main() {
    if let Err(error) = run() {
        eprintln!("gameforgo-desktop: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), BootstrapError> {
    let project_root = std::env::args_os()
        .nth(1)
        .map_or_else(|| PathBuf::from("examples/powder-game"), PathBuf::from);
    let project_name = project_root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("GameForGo Project")
        .to_owned();
    let coordinator_id = format!("desktop-{}", std::process::id());
    let session = start_project(&project_root, &coordinator_id)?;
    let snapshot = session.snapshot().clone();
    let main_commit =
        std::env::var("GAMEFORGO_MAIN_COMMIT").unwrap_or_else(|_| "未確認".to_owned());
    let view = compose_app_shell(
        AppShellContext {
            project_name,
            project_root: project_root.display().to_string(),
            main_commit,
            connection: ConnectionState::Connected,
            projection_revision: snapshot.projection_revision,
            last_synced_at: "起動時".to_owned(),
            is_stale: false,
            inbox_count: 0,
            max_concurrent_task_runs: 2,
        },
        snapshot.development_board,
    );
    let _ = INITIAL_VIEW.set(view);

    dioxus::launch(desktop_root);
    drop(session);
    Ok(())
}

fn desktop_root() -> Element {
    let view = INITIAL_VIEW
        .get()
        .cloned()
        .expect("the application view is initialized before launch");
    rsx! { App { initial_view: view } }
}
