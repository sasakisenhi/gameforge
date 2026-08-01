use std::{
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use dioxus::prelude::*;
use gameforge_application::{
    AppShellContext, AppShellView, ApplicationCommand, ConnectionState,
    compose_app_shell_with_inbox,
};
use gameforge_bootstrap::{BootstrapError, CommandContext, ProjectSession, start_project};
use gameforge_desktop::{App, CommandResult};
use gameforge_runtime::ScheduleConfig;

static INITIAL_VIEW: OnceLock<AppShellView> = OnceLock::new();
static PROJECT_SESSION: OnceLock<Mutex<ProjectSession>> = OnceLock::new();
static NEXT_COMMAND: AtomicU64 = AtomicU64::new(1);

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
    let mut session = start_project(&project_root, &coordinator_id)?;
    let main_commit = resolve_main_commit(&project_root);
    let snapshot = session.run_scheduler(
        next_command_context(&main_commit, "startup-scheduler"),
        single_run_config(),
    )?;
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
            max_concurrent_task_runs: 1,
        },
        snapshot.development_board,
        snapshot.inbox,
    );
    INITIAL_VIEW.set(view).map_err(|_| {
        BootstrapError::Coordinator("initial view is already initialized".to_owned())
    })?;
    PROJECT_SESSION.set(Mutex::new(session)).map_err(|_| {
        BootstrapError::Coordinator("project session is already initialized".to_owned())
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
    let occurred_at = current_timestamp();
    let context = next_command_context(&initial_view.main_commit, "client-command");
    let Some(session) = PROJECT_SESSION.get() else {
        return CommandResult::Failed("Project Sessionが利用できません".to_owned());
    };
    let snapshot = match session
        .lock()
        .map_err(|_| "Project Sessionのロックが破損しました".to_owned())
        .and_then(|mut session| {
            let should_schedule = matches!(command, ApplicationCommand::QueueTaskRun { .. });
            let snapshot = session
                .execute(context, command)
                .map_err(|error| error.to_string())?;
            if should_schedule {
                session
                    .run_scheduler(
                        next_command_context(&initial_view.main_commit, "scheduler-tick"),
                        single_run_config(),
                    )
                    .map_err(|error| error.to_string())
            } else {
                Ok(snapshot)
            }
        }) {
        Ok(snapshot) => snapshot,
        Err(error) => return CommandResult::Failed(error),
    };

    CommandResult::Applied(Box::new(compose_app_shell_with_inbox(
        AppShellContext {
            project_name: initial_view.project_name.clone(),
            project_root: initial_view.project_root.clone(),
            main_commit: initial_view.main_commit.clone(),
            connection: ConnectionState::Connected,
            projection_revision: snapshot.projection_revision,
            last_synced_at: occurred_at,
            is_stale: false,
            inbox_count: snapshot.inbox.len(),
            max_concurrent_task_runs: 1,
        },
        snapshot.development_board,
        snapshot.inbox,
    )))
}

fn next_command_context(base_commit: &str, operation: &str) -> CommandContext {
    CommandContext {
        command_id: format!(
            "desktop-{}-{operation}-{}",
            std::process::id(),
            NEXT_COMMAND.fetch_add(1, Ordering::Relaxed)
        ),
        actor: format!("desktop-{}", std::process::id()),
        occurred_at: current_timestamp(),
        base_commit: base_commit.to_owned(),
    }
}

const fn single_run_config() -> ScheduleConfig {
    ScheduleConfig {
        max_concurrent_task_runs: 1,
    }
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

fn current_timestamp() -> String {
    let milliseconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis());
    format!("unix-ms:{milliseconds}")
}
