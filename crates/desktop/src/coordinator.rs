use std::{
    sync::mpsc::{self, RecvTimeoutError, SyncSender},
    thread::{self, JoinHandle},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use gameforge_application::{ApplicationCommand, RunExecutionPort};
use gameforge_bootstrap::{BootstrapError, CommandContext, ProjectCoordinator, ProjectSnapshot};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoordinatorWorkerContext {
    pub actor: String,
    pub base_commit: String,
}

pub struct CoordinatorWorker {
    sender: SyncSender<WorkerRequest>,
    thread: Option<JoinHandle<()>>,
}

impl CoordinatorWorker {
    /// Applies an application command on the coordinator thread.
    ///
    /// # Errors
    ///
    /// Returns an error when the worker is unavailable or the command is rejected.
    pub fn execute(&self, command: ApplicationCommand) -> Result<ProjectSnapshot, BootstrapError> {
        let (response_sender, response_receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WorkerRequest::Execute {
                command,
                response: response_sender,
            })
            .map_err(|_| BootstrapError::Coordinator("worker stopped".to_owned()))?;
        response_receiver
            .recv()
            .map_err(|_| BootstrapError::Coordinator("worker response closed".to_owned()))?
    }

    /// Reads the latest projection snapshot from the coordinator thread.
    ///
    /// # Errors
    ///
    /// Returns an error when the worker is no longer available.
    pub fn snapshot(&self) -> Result<ProjectSnapshot, BootstrapError> {
        let (response_sender, response_receiver) = mpsc::sync_channel(1);
        self.sender
            .send(WorkerRequest::Snapshot(response_sender))
            .map_err(|_| BootstrapError::Coordinator("worker stopped".to_owned()))?;
        response_receiver
            .recv()
            .map_err(|_| BootstrapError::Coordinator("worker response closed".to_owned()))
    }
}

impl Drop for CoordinatorWorker {
    fn drop(&mut self) {
        let _ = self.sender.send(WorkerRequest::Shutdown);
        if let Some(worker) = self.thread.take() {
            let _ = worker.join();
        }
    }
}

/// Starts the single-writer coordinator and its periodic scheduler loop.
///
/// # Errors
///
/// Returns an error for invalid context, a zero interval, initial scheduling failure, or thread
/// creation failure.
pub fn spawn_coordinator_worker<E>(
    mut coordinator: ProjectCoordinator<E>,
    worker_context: CoordinatorWorkerContext,
    tick_interval: Duration,
) -> Result<CoordinatorWorker, BootstrapError>
where
    E: RunExecutionPort + Send + 'static,
{
    if tick_interval.is_zero() {
        return Err(BootstrapError::Coordinator(
            "worker tick interval must be positive".to_owned(),
        ));
    }
    let mut contexts = ContextGenerator::new(worker_context)?;
    coordinator.tick(&contexts.next("startup"))?;
    let (sender, receiver) = mpsc::sync_channel(32);
    let worker = thread::Builder::new()
        .name("gameforge-coordinator".to_owned())
        .spawn(move || {
            loop {
                match receiver.recv_timeout(tick_interval) {
                    Ok(WorkerRequest::Execute { command, response }) => {
                        let result = coordinator.execute(&contexts.next("client-command"), command);
                        let _ = response.send(result);
                    }
                    Ok(WorkerRequest::Snapshot(response)) => {
                        let _ = response.send(coordinator.snapshot().clone());
                    }
                    Ok(WorkerRequest::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
                    Err(RecvTimeoutError::Timeout) => {
                        if let Err(error) = coordinator.tick(&contexts.next("scheduler-tick")) {
                            eprintln!("gameforge coordinator tick failed: {error}");
                        }
                    }
                }
            }
        })
        .map_err(|error| BootstrapError::Coordinator(error.to_string()))?;
    Ok(CoordinatorWorker {
        sender,
        thread: Some(worker),
    })
}

enum WorkerRequest {
    Execute {
        command: ApplicationCommand,
        response: SyncSender<Result<ProjectSnapshot, BootstrapError>>,
    },
    Snapshot(SyncSender<ProjectSnapshot>),
    Shutdown,
}

struct ContextGenerator {
    actor: String,
    base_commit: String,
    next_id: u64,
}

impl ContextGenerator {
    fn new(context: CoordinatorWorkerContext) -> Result<Self, BootstrapError> {
        if context.actor.trim().is_empty() || context.base_commit.trim().is_empty() {
            return Err(BootstrapError::Coordinator(
                "worker actor and base commit must not be empty".to_owned(),
            ));
        }
        Ok(Self {
            actor: context.actor,
            base_commit: context.base_commit,
            next_id: 1,
        })
    }

    fn next(&mut self, operation: &str) -> CommandContext {
        let command_id = format!("{}-{operation}-{}", self.actor, self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        CommandContext {
            command_id,
            actor: self.actor.clone(),
            occurred_at: current_timestamp(),
            base_commit: self.base_commit.clone(),
        }
    }
}

fn current_timestamp() -> String {
    let milliseconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis());
    format!("unix-ms:{milliseconds}")
}
