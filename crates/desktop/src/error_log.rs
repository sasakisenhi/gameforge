use std::{
    fs::OpenOptions,
    io::Write,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

/// Append an operational error without hiding the original failure.
pub fn append_error(project_root: &Path, operation: &str, error: &str) {
    let directory = project_root.join(".game-dev/runtime");
    if std::fs::create_dir_all(&directory).is_err() {
        return;
    }
    let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(directory.join("gameforge-errors.log"))
    else {
        return;
    };
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    let _ = writeln!(file, "[{timestamp}] operation={operation} error={error}");
}
