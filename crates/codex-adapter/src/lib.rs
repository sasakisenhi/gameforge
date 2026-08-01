//! Codex App Server and Git worktree adapter for Task Run execution.
#![allow(clippy::missing_errors_doc)]

use std::{
    collections::{BTreeMap, HashMap},
    fmt,
    fs::{self, File},
    io::{BufRead, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver, RecvTimeoutError},
    thread,
    time::{Duration, Instant},
};

use gameforge_application::{
    RunExecutionPort, RunExecutionUpdate, RunLaunchDeferral, RunLaunchOutcome, RunLaunchRequest,
    StartedRun,
};
use serde_json::{Value, json};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexRunExecutionConfig {
    pub project_root: PathBuf,
    pub codex_executable: PathBuf,
    pub max_concurrent_task_runs: usize,
    pub startup_timeout: Duration,
}

impl CodexRunExecutionConfig {
    #[must_use]
    pub fn for_project(project_root: impl Into<PathBuf>, max_concurrent_task_runs: usize) -> Self {
        Self {
            project_root: project_root.into(),
            codex_executable: PathBuf::from("codex"),
            max_concurrent_task_runs,
            startup_timeout: Duration::from_secs(15),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexAdapterError(String);

impl fmt::Display for CodexAdapterError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for CodexAdapterError {}

pub struct CodexRunExecution {
    config: CodexRunExecutionConfig,
    active: HashMap<String, ActiveRun>,
}

impl CodexRunExecution {
    pub fn new(mut config: CodexRunExecutionConfig) -> Result<Self, CodexAdapterError> {
        if config.max_concurrent_task_runs == 0 {
            return Err(CodexAdapterError(
                "max_concurrent_task_runs must be positive".to_owned(),
            ));
        }
        config.project_root = config.project_root.canonicalize().map_err(|error| {
            CodexAdapterError(format!(
                "failed to resolve project root {}: {error}",
                config.project_root.display()
            ))
        })?;
        Ok(Self {
            config,
            active: HashMap::new(),
        })
    }

    #[must_use]
    pub fn active_run_count(&self) -> usize {
        self.active.len()
    }

    fn try_start(&mut self, request: &RunLaunchRequest) -> Result<StartedRun, StartFailure> {
        if self.active.contains_key(&request.task_run_id) {
            return Err(StartFailure::agent(format!(
                "Task Run {} is already active",
                request.task_run_id
            )));
        }
        let run_slug = safe_slug(&request.task_run_id);
        let task_slug = safe_slug(&request.task_id);
        if run_slug.is_empty() || task_slug.is_empty() {
            return Err(StartFailure::agent(
                "Task and Task Run IDs must contain an ASCII letter or digit",
            ));
        }
        let task_source = fs::read_to_string(
            self.project_runtime_root()
                .join("tasks")
                .join(format!("{}.md", request.task_id)),
        )
        .map_err(|error| StartFailure::agent(format!("load Task Contract: {error}")))?;
        let prompt = task_prompt(request, &task_source);
        let worktree = self.prepare_worktree(request, &run_slug, &task_slug)?;
        let (protocol_log, stderr_log) = self.create_run_logs(&run_slug)?;
        let StartedProcess {
            child,
            stdin,
            receiver,
            thread_id,
        } = self.start_app_server(&worktree, &prompt, protocol_log, stderr_log)?;

        let started = StartedRun::new(
            format!("resource:{run_slug}"),
            worktree.display().to_string(),
            thread_id.clone(),
        )
        .map_err(|error| StartFailure::agent(error.to_string()))?;
        self.active.insert(
            request.task_run_id.clone(),
            ActiveRun {
                child,
                stdin,
                receiver,
                agent_session_id: thread_id,
                pending_inputs: BTreeMap::new(),
            },
        );
        Ok(started)
    }

    fn prepare_worktree(
        &self,
        request: &RunLaunchRequest,
        run_slug: &str,
        task_slug: &str,
    ) -> Result<PathBuf, StartFailure> {
        let worktrees = self.project_runtime_root().join("worktrees");
        fs::create_dir_all(&worktrees)
            .map_err(|error| StartFailure::worktree(format!("create worktree root: {error}")))?;
        let worktree = worktrees.join(run_slug);
        if worktree.exists() {
            let actual_branch = git_output(&worktree, &["branch", "--show-current"])?;
            let expected_branch = format!("gameforge/{task_slug}/{run_slug}");
            return if actual_branch == expected_branch {
                Ok(worktree)
            } else {
                Err(StartFailure::worktree(format!(
                    "worktree {} uses branch {actual_branch}, expected {expected_branch}",
                    worktree.display()
                )))
            };
        }
        let branch = format!("gameforge/{task_slug}/{run_slug}");
        let branch_ref = format!("refs/heads/{branch}");
        let branch_exists = Command::new("git")
            .arg("-C")
            .arg(&self.config.project_root)
            .args(["show-ref", "--verify", "--quiet"])
            .arg(&branch_ref)
            .status()
            .map_err(|error| StartFailure::worktree(format!("inspect Git branch: {error}")))?
            .success();
        let mut command = Command::new("git");
        command
            .arg("-C")
            .arg(&self.config.project_root)
            .args(["worktree", "add"]);
        if branch_exists {
            command.arg(&worktree).arg(&branch);
        } else {
            command
                .arg("-b")
                .arg(&branch)
                .arg(&worktree)
                .arg(&request.base_commit);
        }
        let output = command
            .output()
            .map_err(|error| StartFailure::worktree(format!("start git worktree: {error}")))?;
        if !output.status.success() {
            return Err(StartFailure::worktree(format!(
                "git worktree add failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        Ok(worktree)
    }

    fn create_run_logs(&self, run_slug: &str) -> Result<(File, File), StartFailure> {
        let run_log_dir = self
            .project_runtime_root()
            .join("runtime/runs")
            .join(run_slug);
        fs::create_dir_all(&run_log_dir)
            .map_err(|error| StartFailure::agent(format!("create Run log directory: {error}")))?;
        let protocol_log = File::create(run_log_dir.join("codex-events.jsonl"))
            .map_err(|error| StartFailure::agent(format!("create Codex event log: {error}")))?;
        let stderr_log = File::create(run_log_dir.join("codex-stderr.log"))
            .map_err(|error| StartFailure::agent(format!("create Codex stderr log: {error}")))?;
        Ok((protocol_log, stderr_log))
    }

    fn start_app_server(
        &self,
        worktree: &Path,
        prompt: &str,
        protocol_log: File,
        stderr_log: File,
    ) -> Result<StartedProcess, StartFailure> {
        let mut child = Command::new(&self.config.codex_executable)
            .args(["app-server", "--listen", "stdio://"])
            .current_dir(worktree)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(stderr_log))
            .spawn()
            .map_err(|error| StartFailure::agent(format!("start Codex App Server: {error}")))?;
        let child_stdin = child
            .stdin
            .take()
            .ok_or_else(|| StartFailure::agent("Codex stdin was not available"))?;
        let child_stdout = child
            .stdout
            .take()
            .ok_or_else(|| StartFailure::agent("Codex stdout was not available"))?;
        let mut stdin = BufWriter::new(child_stdin);
        let receiver = spawn_protocol_reader(child_stdout, protocol_log);

        let startup = (|| {
            send_message(
                &mut stdin,
                &json!({
                    "id": 1,
                    "method": "initialize",
                    "params": {
                        "clientInfo": {"name": "gameforge", "version": env!("CARGO_PKG_VERSION")},
                        "capabilities": {}
                    }
                }),
            )?;
            wait_for_response(&receiver, 1, self.config.startup_timeout)?;
            send_message(&mut stdin, &json!({"method": "initialized"}))?;
            send_message(
                &mut stdin,
                &json!({
                    "id": 2,
                    "method": "thread/start",
                    "params": {
                        "cwd": worktree,
                        "approvalPolicy": "never",
                        "sandbox": "workspace-write"
                    }
                }),
            )?;
            let thread_response = wait_for_response(&receiver, 2, self.config.startup_timeout)?;
            let thread_id = thread_response
                .pointer("/thread/id")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| StartFailure::agent("thread/start returned no thread id"))?
                .to_owned();
            send_message(
                &mut stdin,
                &json!({
                    "id": 3,
                    "method": "turn/start",
                    "params": {
                        "threadId": thread_id,
                        "cwd": worktree,
                        "approvalPolicy": "never",
                        "sandboxPolicy": {
                            "type": "workspaceWrite",
                            "writableRoots": [worktree],
                            "networkAccess": false
                        },
                        "input": [{"type": "text", "text": prompt}]
                    }
                }),
            )?;
            wait_for_response(&receiver, 3, self.config.startup_timeout)?;
            Ok::<_, StartFailure>(thread_id)
        })();
        let thread_id = match startup {
            Ok(thread_id) => thread_id,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };

        Ok(StartedProcess {
            child,
            stdin,
            receiver,
            thread_id,
        })
    }

    fn project_runtime_root(&self) -> PathBuf {
        self.config.project_root.join(".game-dev")
    }
}

impl RunExecutionPort for CodexRunExecution {
    fn start_run(&mut self, request: &RunLaunchRequest) -> RunLaunchOutcome {
        if self.active.len() >= self.config.max_concurrent_task_runs {
            return RunLaunchOutcome::Deferred {
                reason: RunLaunchDeferral::ResourceUnavailable,
                detail: format!(
                    "{} Codex Run(s) already occupy all {} slots",
                    self.active.len(),
                    self.config.max_concurrent_task_runs
                ),
            };
        }
        match self.try_start(request) {
            Ok(started) => RunLaunchOutcome::Started(started),
            Err(error) => RunLaunchOutcome::Deferred {
                reason: error.reason,
                detail: error.detail,
            },
        }
    }

    fn poll_updates(&mut self) -> Vec<RunExecutionUpdate> {
        let mut updates = Vec::new();
        let mut finished = Vec::new();
        for (run_id, active) in &mut self.active {
            while let Ok(message) = active.receiver.try_recv() {
                match process_message(run_id, active, message) {
                    MessageOutcome::Update(update) => {
                        let terminal = matches!(
                            update,
                            RunExecutionUpdate::Completed { .. }
                                | RunExecutionUpdate::Failed { .. }
                        );
                        if terminal {
                            finished.push(run_id.clone());
                        }
                        updates.push(update);
                        if terminal {
                            break;
                        }
                    }
                    MessageOutcome::None => {}
                }
            }
            match active.child.try_wait() {
                Ok(Some(status)) if !finished.contains(run_id) => {
                    finished.push(run_id.clone());
                    updates.push(RunExecutionUpdate::Failed {
                        task_run_id: run_id.clone(),
                        detail: format!("Codex App Server exited unexpectedly: {status}"),
                    });
                }
                Err(error) if !finished.contains(run_id) => {
                    finished.push(run_id.clone());
                    updates.push(RunExecutionUpdate::Failed {
                        task_run_id: run_id.clone(),
                        detail: format!("failed to inspect Codex process: {error}"),
                    });
                }
                Ok(_) | Err(_) => {}
            }
        }
        finished.sort();
        finished.dedup();
        for run_id in finished {
            if let Some(mut active) = self.active.remove(&run_id) {
                stop_process(&mut active.child);
            }
        }
        updates
    }

    fn cancel_run(&mut self, task_run_id: &str) -> Result<(), String> {
        if let Some(mut active) = self.active.remove(task_run_id) {
            stop_process(&mut active.child);
        }
        Ok(())
    }

    fn answer_input(
        &mut self,
        task_run_id: &str,
        request_id: &str,
        answer: &str,
    ) -> Result<(), String> {
        let active = self
            .active
            .get_mut(task_run_id)
            .ok_or_else(|| format!("Task Run {task_run_id} has no active Codex process"))?;
        let pending = active
            .pending_inputs
            .remove(request_id)
            .ok_or_else(|| format!("Codex input request {request_id} is not pending"))?;
        let answers = pending
            .question_ids
            .into_iter()
            .map(|question_id| (question_id, json!({"answers": [answer]})))
            .collect::<serde_json::Map<_, _>>();
        send_message(
            &mut active.stdin,
            &json!({"id": pending.rpc_id, "result": {"answers": answers}}),
        )
        .map_err(|error| error.detail)
    }
}

impl Drop for CodexRunExecution {
    fn drop(&mut self) {
        for active in self.active.values_mut() {
            stop_process(&mut active.child);
        }
    }
}

struct ActiveRun {
    child: Child,
    stdin: BufWriter<ChildStdin>,
    receiver: Receiver<ProtocolMessage>,
    agent_session_id: String,
    pending_inputs: BTreeMap<String, PendingInput>,
}

struct StartedProcess {
    child: Child,
    stdin: BufWriter<ChildStdin>,
    receiver: Receiver<ProtocolMessage>,
    thread_id: String,
}

struct PendingInput {
    rpc_id: Value,
    question_ids: Vec<String>,
}

enum ProtocolMessage {
    Json(Value),
    ReadFailed(String),
}

enum MessageOutcome {
    Update(RunExecutionUpdate),
    None,
}

fn process_message(
    run_id: &str,
    active: &mut ActiveRun,
    message: ProtocolMessage,
) -> MessageOutcome {
    let message = match message {
        ProtocolMessage::Json(message) => message,
        ProtocolMessage::ReadFailed(detail) => {
            return MessageOutcome::Update(RunExecutionUpdate::Failed {
                task_run_id: run_id.to_owned(),
                detail,
            });
        }
    };
    match message.get("method").and_then(Value::as_str) {
        Some("turn/completed") => {
            let status = message
                .pointer("/params/turn/status")
                .and_then(Value::as_str)
                .unwrap_or("failed");
            if status == "completed" {
                MessageOutcome::Update(RunExecutionUpdate::Completed {
                    task_run_id: run_id.to_owned(),
                    agent_session_id: active.agent_session_id.clone(),
                })
            } else {
                let detail = message
                    .pointer("/params/turn/error/message")
                    .and_then(Value::as_str)
                    .unwrap_or("Codex turn did not complete")
                    .to_owned();
                MessageOutcome::Update(RunExecutionUpdate::Failed {
                    task_run_id: run_id.to_owned(),
                    detail,
                })
            }
        }
        Some("item/tool/requestUserInput") => {
            let rpc_id = message.get("id").cloned().unwrap_or(Value::Null);
            let item_id = message
                .pointer("/params/itemId")
                .and_then(Value::as_str)
                .unwrap_or("input");
            let request_id = format!("CODEX-{run_id}-{item_id}");
            let questions = message
                .pointer("/params/questions")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let question_ids = questions
                .iter()
                .filter_map(|question| question.get("id").and_then(Value::as_str))
                .map(str::to_owned)
                .collect();
            let prompt = questions
                .iter()
                .filter_map(|question| question.get("question").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n");
            let prompt = if prompt.trim().is_empty() {
                "Codex requires input before continuing.".to_owned()
            } else {
                prompt
            };
            active.pending_inputs.insert(
                request_id.clone(),
                PendingInput {
                    rpc_id,
                    question_ids,
                },
            );
            MessageOutcome::Update(RunExecutionUpdate::InputRequired {
                task_run_id: run_id.to_owned(),
                request_id,
                prompt,
            })
        }
        Some(method) if method.ends_with("requestApproval") => {
            MessageOutcome::Update(RunExecutionUpdate::Failed {
                task_run_id: run_id.to_owned(),
                detail: format!("Codex requested unsupported approval: {method}"),
            })
        }
        _ => MessageOutcome::None,
    }
}

fn spawn_protocol_reader(
    stdout: impl std::io::Read + Send + 'static,
    mut protocol_log: File,
) -> Receiver<ProtocolMessage> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            match line {
                Ok(line) => {
                    let _ = writeln!(protocol_log, "{line}");
                    match serde_json::from_str(&line) {
                        Ok(message) => {
                            if sender.send(ProtocolMessage::Json(message)).is_err() {
                                break;
                            }
                        }
                        Err(error) => {
                            let _ = sender.send(ProtocolMessage::ReadFailed(format!(
                                "invalid Codex protocol JSON: {error}"
                            )));
                            break;
                        }
                    }
                }
                Err(error) => {
                    let _ = sender.send(ProtocolMessage::ReadFailed(format!(
                        "failed to read Codex protocol: {error}"
                    )));
                    break;
                }
            }
        }
    });
    receiver
}

fn send_message(stdin: &mut BufWriter<ChildStdin>, message: &Value) -> Result<(), StartFailure> {
    serde_json::to_writer(&mut *stdin, message)
        .map_err(|error| StartFailure::agent(format!("encode Codex request: {error}")))?;
    stdin
        .write_all(b"\n")
        .and_then(|()| stdin.flush())
        .map_err(|error| StartFailure::agent(format!("write Codex request: {error}")))
}

fn wait_for_response(
    receiver: &Receiver<ProtocolMessage>,
    request_id: i64,
    timeout: Duration,
) -> Result<Value, StartFailure> {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let message = match receiver.recv_timeout(remaining) {
            Ok(message) => message,
            Err(RecvTimeoutError::Timeout) => {
                return Err(StartFailure::agent(format!(
                    "Codex request {request_id} timed out"
                )));
            }
            Err(RecvTimeoutError::Disconnected) => {
                return Err(StartFailure::agent(
                    "Codex protocol stream closed during startup",
                ));
            }
        };
        match message {
            ProtocolMessage::Json(message)
                if message.get("id").and_then(Value::as_i64) == Some(request_id) =>
            {
                if let Some(error) = message.get("error") {
                    return Err(StartFailure::agent(format!(
                        "Codex request {request_id} failed: {error}"
                    )));
                }
                return message
                    .get("result")
                    .cloned()
                    .ok_or_else(|| StartFailure::agent("Codex response was missing result"));
            }
            ProtocolMessage::ReadFailed(detail) => return Err(StartFailure::agent(detail)),
            ProtocolMessage::Json(_) => {}
        }
    }
}

fn stop_process(child: &mut Child) {
    if child.try_wait().ok().flatten().is_none() {
        let _ = child.kill();
    }
    let _ = child.wait();
}

fn git_output(worktree: &Path, arguments: &[&str]) -> Result<String, StartFailure> {
    let output = Command::new("git")
        .arg("-C")
        .arg(worktree)
        .args(arguments)
        .output()
        .map_err(|error| StartFailure::worktree(format!("start Git command: {error}")))?;
    if !output.status.success() {
        return Err(StartFailure::worktree(format!(
            "git {} failed: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn task_prompt(request: &RunLaunchRequest, source: &str) -> String {
    format!(
        "You are implementing GameForge Task Run {} for Task {} at contract revision {}.\n\nFollow the Task Contract below exactly. Work only inside this Git worktree, use TDD, run the relevant tests, and commit the completed changes on the assigned branch. Do not modify paths outside allowed_paths and test_paths.\n\n{}",
        request.task_run_id, request.task_id, request.contract_revision, source
    )
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

struct StartFailure {
    reason: RunLaunchDeferral,
    detail: String,
}

impl StartFailure {
    fn worktree(detail: impl Into<String>) -> Self {
        Self {
            reason: RunLaunchDeferral::WorktreeUnavailable,
            detail: detail.into(),
        }
    }

    fn agent(detail: impl Into<String>) -> Self {
        Self {
            reason: RunLaunchDeferral::AgentUnavailable,
            detail: detail.into(),
        }
    }
}
