use std::{
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
    sync::OnceLock,
    time::Duration,
};

use dioxus::prelude::*;
use gameforge_application::{
    AppShellContext, AppShellView, ApplicationCommand, ConnectionState,
    compose_app_shell_with_inbox,
};
use gameforge_bootstrap::{BootstrapError, ProjectCoordinator, start_project};
use gameforge_codex_adapter::{CodexRunExecution, CodexRunExecutionConfig};
use gameforge_desktop::{
    App, CommandResult, CoordinatorWorker, CoordinatorWorkerContext, DesktopConfig,
    spawn_coordinator_worker,
};
use gameforge_local_check_adapter::{LocalCheckRunner, LocalVerificationConfig};
use gameforge_runtime::ScheduleConfig;

static INITIAL_VIEW: OnceLock<AppShellView> = OnceLock::new();
static COORDINATOR: OnceLock<CoordinatorWorker> = OnceLock::new();
static DESKTOP_CONFIG: OnceLock<DesktopConfig> = OnceLock::new();

fn main() {
    if let Err(error) = run() {
        eprintln!("gameforge-desktop: {error}");
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
        .unwrap_or("GameForge Project")
        .to_owned();
    let coordinator_id = format!("desktop-{}", std::process::id());
    let config = DesktopConfig::from_env()
        .map_err(|error| BootstrapError::Coordinator(error.to_string()))?;
    let main_commit = resolve_main_commit(&project_root);
    let session = start_project(&project_root, &coordinator_id)?;
    let execution = CodexRunExecution::new(CodexRunExecutionConfig::for_project(
        &project_root,
        config.max_concurrent_task_runs(),
    ))
    .map_err(|error| BootstrapError::RunExecution(error.to_string()))?;
    let verification = LocalCheckRunner::new(LocalVerificationConfig::for_project(&project_root))
        .map_err(|error| BootstrapError::LocalVerification(error.to_string()))?;
    let coordinator = ProjectCoordinator::with_verification(
        session,
        execution,
        verification,
        ScheduleConfig {
            max_concurrent_task_runs: config.max_concurrent_task_runs(),
        },
    );
    let worker = spawn_coordinator_worker(
        coordinator,
        CoordinatorWorkerContext {
            actor: coordinator_id,
            base_commit: main_commit.clone(),
        },
        Duration::from_millis(250),
    )?;
    let snapshot = worker.snapshot()?;
    let view = compose_app_shell_with_inbox(
        AppShellContext {
            project_name,
            project_root: project_root.display().to_string(),
            main_commit,
            connection: ConnectionState::Connected,
            projection_revision: snapshot.projection_revision,
            last_synced_at: "起動時".to_owned(),
            is_stale: false,
            inbox_count: snapshot.inbox.len(),
            max_concurrent_task_runs: config.max_concurrent_task_runs(),
        },
        snapshot.development_board,
        snapshot.inbox,
    );
    INITIAL_VIEW.set(view).map_err(|_| {
        BootstrapError::Coordinator("initial view is already initialized".to_owned())
    })?;
    COORDINATOR.set(worker).map_err(|_| {
        BootstrapError::Coordinator("coordinator worker is already initialized".to_owned())
    })?;
    DESKTOP_CONFIG.set(config).map_err(|_| {
        BootstrapError::Coordinator("desktop config is already initialized".to_owned())
    })?;

    dioxus::launch(desktop_root);
    Ok(())
}

fn desktop_root() -> Element {
    let view = INITIAL_VIEW
        .get()
        .cloned()
        .expect("the application view is initialized before launch");
    rsx! {
        App {
            initial_view: view,
            on_command: execute_application_command,
        }
    }
}

fn execute_application_command(command: ApplicationCommand) -> CommandResult {
    let Some(initial_view) = INITIAL_VIEW.get() else {
        return CommandResult::Failed("初期Viewが利用できません".to_owned());
    };
    let Some(coordinator) = COORDINATOR.get() else {
        return CommandResult::Failed("Coordinatorが利用できません".to_owned());
    };
    let snapshot = match coordinator.execute(command) {
        Ok(snapshot) => snapshot,
        Err(error) => return CommandResult::Failed(error.to_string()),
    };
    let Some(config) = DESKTOP_CONFIG.get().copied() else {
        return CommandResult::Failed("Desktop設定が利用できません".to_owned());
    };

    CommandResult::Applied(Box::new(compose_app_shell_with_inbox(
        AppShellContext {
            project_name: initial_view.project_name.clone(),
            project_root: initial_view.project_root.clone(),
            main_commit: initial_view.main_commit.clone(),
            connection: ConnectionState::Connected,
            projection_revision: snapshot.projection_revision,
            last_synced_at: "command applied".to_owned(),
            is_stale: false,
            inbox_count: snapshot.inbox.len(),
            max_concurrent_task_runs: config.max_concurrent_task_runs(),
        },
        snapshot.development_board,
        snapshot.inbox,
    )))
}

fn resolve_main_commit(project_root: &Path) -> String {
    if let Ok(commit) = std::env::var("GAMEFORGE_MAIN_COMMIT")
        && valid_commit(&commit)
    {
        return commit.to_ascii_lowercase();
    }
    ProcessCommand::new("git")
        .arg("-C")
        .arg(project_root)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|commit| commit.trim().to_owned())
        .filter(|commit| valid_commit(commit))
        .unwrap_or_else(|| "未確認".to_owned())
}

fn valid_commit(commit: &str) -> bool {
    matches!(commit.len(), 40 | 64) && commit.bytes().all(|byte| byte.is_ascii_hexdigit())
}
