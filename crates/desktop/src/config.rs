use std::fmt;

use gameforge_runtime::DEFAULT_MAX_CONCURRENT_TASK_RUNS;

pub const MAX_CONCURRENT_TASK_RUNS_ENV: &str = "GAMEFORGE_MAX_CONCURRENT_TASK_RUNS";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopConfig {
    max_concurrent_task_runs: usize,
}

impl DesktopConfig {
    /// Loads the Desktop runtime settings from environment variables.
    ///
    /// # Errors
    ///
    /// Returns an error when the concurrency setting is not a positive integer.
    pub fn from_env() -> Result<Self, DesktopConfigError> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    /// Loads settings with a caller-supplied environment lookup.
    ///
    /// # Errors
    ///
    /// Returns an error when the concurrency setting is not a positive integer.
    pub fn from_lookup(
        mut lookup: impl FnMut(&str) -> Option<String>,
    ) -> Result<Self, DesktopConfigError> {
        let max_concurrent_task_runs = lookup(MAX_CONCURRENT_TASK_RUNS_ENV).map_or(
            Ok(DEFAULT_MAX_CONCURRENT_TASK_RUNS),
            |value| {
                value
                    .parse::<usize>()
                    .ok()
                    .filter(|maximum| *maximum > 0)
                    .ok_or(DesktopConfigError { value })
            },
        )?;
        Ok(Self {
            max_concurrent_task_runs,
        })
    }

    #[must_use]
    pub const fn max_concurrent_task_runs(self) -> usize {
        self.max_concurrent_task_runs
    }
}

impl Default for DesktopConfig {
    fn default() -> Self {
        Self {
            max_concurrent_task_runs: DEFAULT_MAX_CONCURRENT_TASK_RUNS,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopConfigError {
    value: String,
}

impl fmt::Display for DesktopConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{MAX_CONCURRENT_TASK_RUNS_ENV} must be a positive integer, got {:?}",
            self.value
        )
    }
}

impl std::error::Error for DesktopConfigError {}
