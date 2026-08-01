use gameforge_application::{ApplicationCommand, RunExecutionPort, RunExecutionUpdate};
use gameforge_runtime::ScheduleConfig;

use crate::{BootstrapError, CommandContext, ProjectSession, ProjectSnapshot};

pub struct ProjectCoordinator<E> {
    session: ProjectSession,
    execution: E,
    schedule_config: ScheduleConfig,
}

impl<E: RunExecutionPort> ProjectCoordinator<E> {
    #[must_use]
    pub const fn new(
        session: ProjectSession,
        execution: E,
        schedule_config: ScheduleConfig,
    ) -> Self {
        Self {
            session,
            execution,
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
            self.execution
                .cancel_run(&run_id)
                .map_err(BootstrapError::RunExecution)?;
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
                } => self.session.record_agent_completed(
                    &update_context,
                    &task_run_id,
                    &agent_session_id,
                )?,
                RunExecutionUpdate::Failed {
                    task_run_id,
                    detail,
                } => self
                    .session
                    .record_agent_failed(&update_context, &task_run_id, &detail)?,
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
                    )?
                }
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
