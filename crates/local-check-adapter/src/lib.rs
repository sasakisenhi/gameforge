//! Asynchronous Git and process adapter for Task Run local verification.
#![allow(clippy::missing_errors_doc)]

use std::{
    collections::{BTreeSet, HashMap},
    ffi::OsString,
    fmt,
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Stdio},
    sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError},
    thread::{self, JoinHandle},
    time::Duration,
};

use gameforge_application::{
    LocalVerificationPort, LocalVerificationRequest, LocalVerificationUpdate,
};

const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalCheckCommand {
    name: String,
    program: PathBuf,
    args: Vec<OsString>,
    final_suite: bool,
}

impl LocalCheckCommand {
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        program: impl Into<PathBuf>,
        args: impl IntoIterator<Item = impl Into<OsString>>,
        final_suite: bool,
    ) -> Self {
        Self {
            name: name.into(),
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
            final_suite,
        }
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn is_final_suite(&self) -> bool {
        self.final_suite
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalVerificationConfig {
    pub project_root: PathBuf,
    pub commands: Vec<LocalCheckCommand>,
}

impl LocalVerificationConfig {
    #[must_use]
    pub fn for_project(project_root: impl Into<PathBuf>) -> Self {
        Self {
            project_root: project_root.into(),
            commands: vec![
                LocalCheckCommand::new("format", "cargo", ["fmt", "--all", "--", "--check"], false),
                LocalCheckCommand::new(
                    "lint",
                    "cargo",
                    [
                        "clippy",
                        "--workspace",
                        "--all-targets",
                        "-j",
                        "1",
                        "--",
                        "-D",
                        "warnings",
                    ],
                    false,
                ),
                LocalCheckCommand::new("test", "cargo", ["test", "--workspace", "-j", "1"], true),
                LocalCheckCommand::new(
                    "build",
                    "cargo",
                    ["build", "--workspace", "-j", "1"],
                    false,
                ),
            ],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalCheckAdapterError(String);

impl fmt::Display for LocalCheckAdapterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for LocalCheckAdapterError {}

pub struct LocalCheckRunner {
    config: LocalVerificationConfig,
    active: HashMap<String, ActiveVerification>,
}

impl LocalCheckRunner {
    pub fn new(mut config: LocalVerificationConfig) -> Result<Self, LocalCheckAdapterError> {
        config.project_root = config.project_root.canonicalize().map_err(|error| {
            LocalCheckAdapterError(format!(
                "failed to resolve project root {}: {error}",
                config.project_root.display()
            ))
        })?;
        validate_commands(&config.commands)?;
        Ok(Self {
            config,
            active: HashMap::new(),
        })
    }

    #[must_use]
    pub fn active_verification_count(&self) -> usize {
        self.active.len()
    }

    fn try_start(&mut self, request: &LocalVerificationRequest) -> Result<(), String> {
        validate_request(request)?;
        if self.active.contains_key(&request.task_run_id) {
            return Err(format!(
                "Local Verification is already active for {}",
                request.task_run_id
            ));
        }
        let run_slug = safe_slug(&request.task_run_id);
        let expected_worktree = self
            .config
            .project_root
            .join(".game-dev/worktrees")
            .join(&run_slug);
        let worktree = PathBuf::from(&request.worktree_path)
            .canonicalize()
            .map_err(|error| format!("resolve Local Verification worktree: {error}"))?;
        let expected_worktree = expected_worktree
            .canonicalize()
            .map_err(|error| format!("resolve expected Task Run worktree: {error}"))?;
        if worktree != expected_worktree {
            return Err(format!(
                "Local Verification worktree {} does not match expected {}",
                worktree.display(),
                expected_worktree.display()
            ));
        }

        let log_path = self
            .config
            .project_root
            .join(".game-dev/runtime/runs")
            .join(&run_slug)
            .join("local-checks.log");
        if let Some(parent) = log_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("create Local Verification log directory: {error}"))?;
        }
        let log = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&log_path)
            .map_err(|error| format!("create Local Verification log: {error}"))?;
        let (cancel_sender, cancel_receiver) = mpsc::sync_channel(1);
        let (result_sender, result_receiver) = mpsc::sync_channel(1);
        let worker_request = request.clone();
        let commands = self.config.commands.clone();
        let thread = thread::Builder::new()
            .name(format!("gameforge-local-check-{run_slug}"))
            .spawn(move || {
                let outcome = run_verification(
                    &worker_request,
                    &worktree,
                    &log_path,
                    log,
                    &commands,
                    &cancel_receiver,
                );
                let _ = result_sender.send(outcome);
            })
            .map_err(|error| format!("start Local Verification worker: {error}"))?;
        self.active.insert(
            request.task_run_id.clone(),
            ActiveVerification {
                cancel_sender,
                result_receiver,
                thread: Some(thread),
            },
        );
        Ok(())
    }

    fn finish(&mut self, run_id: &str) {
        if let Some(mut active) = self.active.remove(run_id)
            && let Some(thread) = active.thread.take()
        {
            let _ = thread.join();
        }
    }
}

impl LocalVerificationPort for LocalCheckRunner {
    fn start_verification(&mut self, request: &LocalVerificationRequest) -> Result<(), String> {
        self.try_start(request)
    }

    fn is_verification_active(&self, task_run_id: &str) -> bool {
        self.active.contains_key(task_run_id)
    }

    fn poll_updates(&mut self) -> Vec<LocalVerificationUpdate> {
        let mut completed = Vec::new();
        let mut updates = Vec::new();
        let mut run_ids = self.active.keys().cloned().collect::<Vec<_>>();
        run_ids.sort();
        for run_id in run_ids {
            let outcome = match self.active[&run_id].result_receiver.try_recv() {
                Ok(outcome) => Some(outcome),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => Some(WorkerOutcome::Failed(
                    "Local Verification worker exited without a result".to_owned(),
                )),
            };
            let Some(outcome) = outcome else {
                continue;
            };
            completed.push(run_id.clone());
            match outcome {
                WorkerOutcome::Passed {
                    head_commit,
                    changed_paths,
                    completed_checks,
                    final_suite_passed,
                } => updates.push(LocalVerificationUpdate::Passed {
                    task_run_id: run_id,
                    head_commit,
                    changed_paths,
                    completed_checks,
                    final_suite_passed,
                }),
                WorkerOutcome::Failed(detail) => {
                    updates.push(LocalVerificationUpdate::Failed {
                        task_run_id: run_id,
                        detail,
                    });
                }
                WorkerOutcome::Cancelled => {}
            }
        }
        for run_id in completed {
            self.finish(&run_id);
        }
        updates
    }

    fn cancel_verification(&mut self, task_run_id: &str) -> Result<(), String> {
        if let Some(active) = self.active.get(task_run_id) {
            let _ = active.cancel_sender.send(());
        }
        self.finish(task_run_id);
        Ok(())
    }
}

impl Drop for LocalCheckRunner {
    fn drop(&mut self) {
        let run_ids = self.active.keys().cloned().collect::<Vec<_>>();
        for run_id in &run_ids {
            if let Some(active) = self.active.get(run_id) {
                let _ = active.cancel_sender.send(());
            }
        }
        for run_id in run_ids {
            self.finish(&run_id);
        }
    }
}

struct ActiveVerification {
    cancel_sender: SyncSender<()>,
    result_receiver: Receiver<WorkerOutcome>,
    thread: Option<JoinHandle<()>>,
}

enum WorkerOutcome {
    Passed {
        head_commit: String,
        changed_paths: Vec<String>,
        completed_checks: Vec<String>,
        final_suite_passed: bool,
    },
    Failed(String),
    Cancelled,
}

fn run_verification(
    request: &LocalVerificationRequest,
    worktree: &Path,
    log_path: &Path,
    mut log: File,
    commands: &[LocalCheckCommand],
    cancel: &Receiver<()>,
) -> WorkerOutcome {
    match run_verification_inner(request, worktree, log_path, &mut log, commands, cancel) {
        Ok(outcome) => outcome,
        Err(error) => WorkerOutcome::Failed(error),
    }
}

fn run_verification_inner(
    request: &LocalVerificationRequest,
    worktree: &Path,
    log_path: &Path,
    log: &mut File,
    commands: &[LocalCheckCommand],
    cancel: &Receiver<()>,
) -> Result<WorkerOutcome, String> {
    writeln!(log, "Task Run: {}", request.task_run_id)
        .map_err(|error| format!("write Local Verification log: {error}"))?;
    let repository_root = git_text(worktree, ["rev-parse", "--show-toplevel"], log)?;
    let repository_root = PathBuf::from(repository_root)
        .canonicalize()
        .map_err(|error| format!("resolve Git worktree root: {error}"))?;
    if repository_root != worktree {
        return Err(format!(
            "Git worktree root {} does not match requested {}",
            repository_root.display(),
            worktree.display()
        ));
    }
    let status = git_bytes(
        worktree,
        ["status", "--porcelain=v1", "-z", "--untracked-files=all"],
        log,
    )?;
    if !status.is_empty() {
        return Err(format!(
            "Local Verification requires a clean committed worktree; see {}",
            log_path.display()
        ));
    }
    let ancestor = Command::new("git")
        .arg("-C")
        .arg(worktree)
        .args(["merge-base", "--is-ancestor", &request.base_commit, "HEAD"])
        .status()
        .map_err(|error| format!("inspect Task Run base commit: {error}"))?;
    if !ancestor.success() {
        return Err(format!(
            "Task Run base commit {} is not an ancestor of HEAD",
            request.base_commit
        ));
    }
    let head_commit = git_text(worktree, ["rev-parse", "HEAD"], log)?;
    if !valid_commit(&head_commit) {
        return Err(format!("Git returned invalid HEAD commit {head_commit:?}"));
    }
    let changed_paths = changed_paths(worktree, &request.base_commit, log)?;
    if changed_paths.is_empty() {
        return Err("Task Run produced no committed changes".to_owned());
    }

    let mut completed_checks = Vec::with_capacity(commands.len());
    let mut final_suite_passed = false;
    for check in commands {
        match run_check(check, worktree, log, cancel)? {
            CheckOutcome::Passed => {
                completed_checks.push(check.name.clone());
                final_suite_passed |= check.final_suite;
            }
            CheckOutcome::Cancelled => return Ok(WorkerOutcome::Cancelled),
            CheckOutcome::Failed(status) => {
                return Err(format!(
                    "Local check {} failed with {}; see {}",
                    check.name,
                    display_status(status),
                    log_path.display()
                ));
            }
        }
    }
    Ok(WorkerOutcome::Passed {
        head_commit,
        changed_paths,
        completed_checks,
        final_suite_passed,
    })
}

enum CheckOutcome {
    Passed,
    Failed(ExitStatus),
    Cancelled,
}

fn run_check(
    check: &LocalCheckCommand,
    worktree: &Path,
    log: &mut File,
    cancel: &Receiver<()>,
) -> Result<CheckOutcome, String> {
    writeln!(
        log,
        "\n== {} ==\nprogram: {}\nargs: {:?}",
        check.name,
        check.program.display(),
        check.args
    )
    .map_err(|error| format!("write Local Verification log: {error}"))?;
    log.flush()
        .map_err(|error| format!("flush Local Verification log: {error}"))?;
    let stdout = log
        .try_clone()
        .map_err(|error| format!("clone Local Verification log: {error}"))?;
    let stderr = log
        .try_clone()
        .map_err(|error| format!("clone Local Verification log: {error}"))?;
    let mut child = Command::new(&check.program)
        .args(&check.args)
        .current_dir(worktree)
        .env("CARGO_TERM_COLOR", "never")
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .spawn()
        .map_err(|error| format!("start local check {}: {error}", check.name))?;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(CheckOutcome::Passed),
            Ok(Some(status)) => return Ok(CheckOutcome::Failed(status)),
            Ok(None) => {}
            Err(error) => return Err(format!("inspect local check {}: {error}", check.name)),
        }
        match cancel.recv_timeout(PROCESS_POLL_INTERVAL) {
            Ok(()) | Err(RecvTimeoutError::Disconnected) => {
                let _ = child.kill();
                let _ = child.wait();
                return Ok(CheckOutcome::Cancelled);
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
}

fn changed_paths(
    worktree: &Path,
    base_commit: &str,
    log: &mut File,
) -> Result<Vec<String>, String> {
    let range = format!("{base_commit}..HEAD");
    let output = git_bytes(
        worktree,
        [
            "diff",
            "--name-status",
            "-z",
            "--find-renames",
            &range,
            "--",
        ],
        log,
    )?;
    parse_name_status(&output)
}

fn parse_name_status(output: &[u8]) -> Result<Vec<String>, String> {
    let mut fields = output
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty());
    let mut paths = BTreeSet::new();
    while let Some(status) = fields.next() {
        let status = std::str::from_utf8(status)
            .map_err(|_| "Git diff returned a non-UTF-8 status".to_owned())?;
        let path_count = usize::from(status.starts_with('R') || status.starts_with('C')) + 1;
        for _ in 0..path_count {
            let path = fields
                .next()
                .ok_or_else(|| format!("Git diff omitted a path for status {status}"))?;
            let path = std::str::from_utf8(path)
                .map_err(|_| "Git diff returned a non-UTF-8 path".to_owned())?;
            paths.insert(path.to_owned());
        }
    }
    Ok(paths.into_iter().collect())
}

fn git_text<const N: usize>(
    worktree: &Path,
    args: [&str; N],
    log: &mut File,
) -> Result<String, String> {
    let bytes = git_bytes(worktree, args, log)?;
    String::from_utf8(bytes)
        .map(|value| value.trim().to_owned())
        .map_err(|_| "Git returned non-UTF-8 text".to_owned())
}

fn git_bytes<const N: usize>(
    worktree: &Path,
    args: [&str; N],
    log: &mut File,
) -> Result<Vec<u8>, String> {
    writeln!(log, "git {args:?}")
        .map_err(|error| format!("write Local Verification log: {error}"))?;
    let output = Command::new("git")
        .arg("-C")
        .arg(worktree)
        .args(args)
        .output()
        .map_err(|error| format!("run Git inspection: {error}"))?;
    if !output.stderr.is_empty() {
        log.write_all(&output.stderr)
            .map_err(|error| format!("write Git diagnostic: {error}"))?;
    }
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(format!(
            "Git inspection failed with {}",
            display_status(output.status)
        ))
    }
}

fn validate_commands(commands: &[LocalCheckCommand]) -> Result<(), LocalCheckAdapterError> {
    if commands.is_empty() {
        return Err(LocalCheckAdapterError(
            "at least one local check command is required".to_owned(),
        ));
    }
    if !commands.iter().any(|command| command.final_suite) {
        return Err(LocalCheckAdapterError(
            "one local check command must be marked as the final suite".to_owned(),
        ));
    }
    let mut names = BTreeSet::new();
    for command in commands {
        if command.name.trim().is_empty() || command.program.as_os_str().is_empty() {
            return Err(LocalCheckAdapterError(
                "local check names and programs must not be empty".to_owned(),
            ));
        }
        if !names.insert(&command.name) {
            return Err(LocalCheckAdapterError(format!(
                "duplicate local check name {}",
                command.name
            )));
        }
    }
    Ok(())
}

fn validate_request(request: &LocalVerificationRequest) -> Result<(), String> {
    for (field, value) in [
        ("task_run_id", request.task_run_id.as_str()),
        ("task_id", request.task_id.as_str()),
        ("base_commit", request.base_commit.as_str()),
        ("worktree_path", request.worktree_path.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(format!("{field} must not be empty"));
        }
    }
    if safe_slug(&request.task_run_id).is_empty() {
        return Err("task_run_id must contain an ASCII letter or digit".to_owned());
    }
    if !valid_commit(&request.base_commit) {
        return Err("base_commit must be a 40- or 64-character hexadecimal commit".to_owned());
    }
    Ok(())
}

fn safe_slug(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            result.push(character.to_ascii_lowercase());
        } else if !result.ends_with('-') {
            result.push('-');
        }
    }
    result.trim_matches('-').to_owned()
}

fn valid_commit(commit: &str) -> bool {
    matches!(commit.len(), 40 | 64) && commit.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn display_status(status: ExitStatus) -> String {
    status.code().map_or_else(
        || "termination by signal".to_owned(),
        |code| format!("exit code {code}"),
    )
}

#[cfg(test)]
mod tests {
    use super::parse_name_status;

    #[test]
    fn includes_both_sides_of_renames_in_scope_input() {
        let paths =
            parse_name_status(b"R100\0old/path.rs\0new/path.rs\0M\0same/path.rs\0").unwrap();

        assert_eq!(paths, ["new/path.rs", "old/path.rs", "same/path.rs"]);
    }
}
