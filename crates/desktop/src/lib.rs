//! Dioxus desktop client, runtime bridge, and pure UI state transitions.

mod config;
mod coordinator;
mod error_log;
mod shell;
mod state;
mod views;

pub use config::{DesktopConfig, DesktopConfigError, MAX_CONCURRENT_TASK_RUNS_ENV};
pub use coordinator::{CoordinatorWorker, CoordinatorWorkerContext, spawn_coordinator_worker};
pub use error_log::append_error;
pub use shell::{App, render_app, render_inbox};
pub use state::{
    ColorTheme, CommandEffect, CommandResult, Route, TaskFilter, UiAction, UiState,
    execute_board_intent, execute_inbox_intent, reduce_ui_state,
};
