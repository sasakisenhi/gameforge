//! Headless client entry points.
#![allow(clippy::missing_errors_doc)]

use std::{fmt, io::Write, path::Path};

use gameforgo_bootstrap::{rebuild_project, validate_project};

const USAGE: &str = "使い方: gameforgo <validate|rebuild> PROJECT_ROOT";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliError {
    Usage(String),
    Operation(String),
    Output(String),
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(message) | Self::Operation(message) | Self::Output(message) => {
                formatter.write_str(message)
            }
        }
    }
}

impl std::error::Error for CliError {}

impl From<std::io::Error> for CliError {
    fn from(error: std::io::Error) -> Self {
        Self::Output(error.to_string())
    }
}

pub fn run<I, S, W>(arguments: I, output: &mut W) -> Result<(), CliError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
    W: Write,
{
    let mut arguments = arguments.into_iter();
    let command = arguments
        .next()
        .ok_or_else(|| CliError::Usage(USAGE.to_owned()))?;
    let project_root = arguments
        .next()
        .ok_or_else(|| CliError::Usage(USAGE.to_owned()))?;
    if arguments.next().is_some() {
        return Err(CliError::Usage(USAGE.to_owned()));
    }
    let project_root = Path::new(project_root.as_ref());

    match command.as_ref() {
        "validate" => {
            let validation = validate_project(project_root)
                .map_err(|error| CliError::Operation(error.to_string()))?;
            writeln!(output, "検証成功: {}件のTask", validation.task_count)
                .map_err(CliError::from)?;
            writeln!(
                output,
                "統合順序: {}",
                validation.integration_order.join(" -> ")
            )
            .map_err(CliError::from)?;
        }
        "rebuild" => {
            let instance_id = format!("cli-{}", std::process::id());
            let snapshot = rebuild_project(project_root, &instance_id)
                .map_err(|error| CliError::Operation(error.to_string()))?;
            writeln!(
                output,
                "Read Model再構築完了: revision {}",
                snapshot.projection_revision
            )
            .map_err(CliError::from)?;
            for row in snapshot.development_board {
                writeln!(
                    output,
                    "{} | {} | run={}",
                    row.task_id,
                    row.task_status,
                    row.current_run_id.as_deref().unwrap_or("—")
                )
                .map_err(CliError::from)?;
            }
        }
        _ => return Err(CliError::Usage(USAGE.to_owned())),
    }
    Ok(())
}
