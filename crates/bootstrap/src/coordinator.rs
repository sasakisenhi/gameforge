use gameforge_application::{
    ApplicationCommand, LocalVerificationPort, LocalVerificationUpdate, RunExecutionPort,
    RunExecutionUpdate,
};
use gameforge_runtime::ScheduleConfig;

use crate::{BootstrapError, CommandContext, ProjectSession, ProjectSnapshot};

pub struct ProjectCoordinator<E, V> {
    session: ProjectSession,
    execution: E,
    verification: V,
    schedule_config: ScheduleConfig,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct UnavailableLocalVerification;

impl LocalVerificationPort for UnavailableLocalVerification {
    fn start_verification(
        &mut self,
        _request: &gameforge_application::LocalVerificationRequest,
    ) -> Result<(), String> {
        Err("Local Verification adapter is not configured".to_owned())
    }
}

impl<E: RunExecutionPort> ProjectCoordinator<E, UnavailableLocalVerification> {
    #[must_use]
    pub const fn new(
        session: ProjectSession,
        execution: E,
        schedule_config: ScheduleConfig,
    ) -> Self {
        Self {
            session,
            execution,
            verification: UnavailableLocalVerification,
            schedule_config,
        }
    }
}

impl<E: RunExecutionPort, V: LocalVerificationPort> ProjectCoordinator<E, V> {
    #[must_use]
    pub const fn with_verification(
        session: ProjectSession,
        execution: E,
        verification: V,
        schedule_config: ScheduleConfig,
    ) -> Self {
        Self {
            session,
            execution,
            verification,
            schedule_config,
        }
    }

    #[must_use]
    pub const fn snapshot(&self) -> &ProjectSnapshot {
        self.session.snapshot()
    }

    pub fn execute(
        &mut self,
        context: &CommandContext,
        command: ApplicationCommand,
    ) -> Result<ProjectSnapshot, BootstrapError> {
        let cancelled_run = match &command {
            ApplicationCommand::CancelTaskRun { task_run_id, .. } => Some(task_run_id.clone()),
            _ => None,
        };
        let answered_input = match &command {
            ApplicationCommand::AnswerInputRequest {
                request_id, answer, ..
            } => self
                .session
                .snapshot()
                .inbox
                .iter()
                .find(|item| item.request_id == *request_id)
                .map(|item| (item.task_run_id.clone(), request_id.clone(), answer.clone())),
            _ => None,
        };

        self.session.execute(context.clone(), command)?;
        if let Some(run_id) = cancelled_run {
            let execution_result = self.execution.cancel_run(&run_id);
            let verification_result = self.verification.cancel_verification(&run_id);
            execution_result.map_err(BootstrapError::RunExecution)?;
            verification_result.map_err(BootstrapError::LocalVerification)?;
        }
        if let Some((run_id, request_id, answer)) = answered_input {
            self.execution
                .answer_input(&run_id, &request_id, &answer)
                .map_err(BootstrapError::RunExecution)?;
        }
        self.tick(&context.child("post-command"))
    }

    pub fn tick(&mut self, context: &CommandContext) -> Result<ProjectSnapshot, BootstrapError> {
        for (index, update) in self.execution.poll_updates().into_iter().enumerate() {
            let update_context = context.child(&format!("update-{index}"));
            match update {
                RunExecutionUpdate::Completed {
                    task_run_id,
                    agent_session_id,
                    red_evidence_present,
                    green_evidence_present,
                } => {
                    self.session.record_agent_completed(
                        &update_context,
                        &task_run_id,
                        &agent_session_id,
                        red_evidence_present,
                        green_evidence_present,
                    )?;
                    let request = self.session.local_verification_request(&task_run_id)?;
                    if let Err(error) = self.verification.start_verification(&request) {
                        self.session.record_local_verification_failed(
                            &update_context.child("start-failed"),
                            &task_run_id,
                            &error,
                        )?;
                    }
                }
                RunExecutionUpdate::Failed {
                    task_run_id,
                    detail,
                } => {
                    self.session
                        .record_agent_failed(&update_context, &task_run_id, &detail)?;
                }
                RunExecutionUpdate::InputRequired {
                    task_run_id,
                    request_id,
                    prompt,
                } => {
                    let revision = self.session.snapshot().projection_revision;
                    self.session.record_input_required(
                        update_context,
                        &task_run_id,
                        &request_id,
                        &prompt,
                        revision,
                    )?;
                }
            }
        }

        for (index, update) in self.verification.poll_updates().into_iter().enumerate() {
            let update_context = context.child(&format!("verification-update-{index}"));
            match update {
                LocalVerificationUpdate::Passed {
                    task_run_id,
                    head_commit,
                    changed_paths,
                    completed_checks,
                    final_suite_passed,
                } => self.session.record_local_verification_passed(
                    &update_context,
                    &task_run_id,
                    &head_commit,
                    &changed_paths,
                    &completed_checks,
                    final_suite_passed,
                )?,
                LocalVerificationUpdate::Failed {
                    task_run_id,
                    detail,
                } => self.session.record_local_verification_failed(
                    &update_context,
                    &task_run_id,
                    &detail,
                )?,
            };
        }

        self.session
            .run_scheduler(context.child("scheduler"), self.schedule_config)?;
        for (index, run_id) in self.session.preparing_run_ids().into_iter().enumerate() {
            self.session.run_supervisor(
                &context.child(&format!("supervisor-{index}")),
                &run_id,
                &mut self.execution,
            )?;
        }
        Ok(self.session.snapshot().clone())
    }
}
