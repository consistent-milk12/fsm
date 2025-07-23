//! src/tasks/filename_search_task.rs
//! ============================================================================
//! # Filename Search Task: Background recursive filename search
//!
//! Spawns an async find/fd process to recursively search for filenames,
//! converts each hit into `ObjectInfo`, and reports results back to the UI.

use std::path::PathBuf;
use std::process::Stdio;

use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::mpsc::UnboundedSender,
};

use crate::{
    controller::{actions::Action, event_loop::TaskResult},
    fs::object_info::ObjectInfo,
};

pub fn filename_search_task(
    task_id: u64,
    pattern: String,
    search_path: PathBuf,
    task_tx: UnboundedSender<TaskResult>,
    action_tx: UnboundedSender<Action>,
) {
    tokio::spawn(async move {
        let mut results = Vec::<ObjectInfo>::new();

        // Try to use 'fd' first (faster), fallback to 'find'
        let mut child = if let Ok(child) = Command::new("fd")
            .arg("--type")
            .arg("f") // files and directories
            .arg("--type")
            .arg("d")
            .arg(&pattern)
            .arg(&search_path)
            .kill_on_drop(true)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        {
            child
        } else {
            // Fallback to find command
            match Command::new("find")
                .arg(&search_path)
                .arg("-iname")
                .arg(format!("*{pattern}*"))
                .kill_on_drop(true)
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
            {
                Ok(c) => c,
                Err(e) => {
                    let _ = task_tx.send(TaskResult::error(
                        task_id,
                        format!("failed to spawn filename search: {e}"),
                    ));
                    return;
                }
            }
        };

        // Stream command output line-by-line
        let stdout = child.stdout.take().expect("stdout must be piped");
        let mut reader = BufReader::new(stdout).lines();

        while let Ok(Some(line)) = reader.next_line().await {
            let file_path = PathBuf::from(line.trim());
            if file_path.exists() {
                match ObjectInfo::from_path(&file_path).await {
                    Ok(info) => results.push(info),
                    Err(e) => tracing::error!(
                        "could not stat filename search hit {}: {e}",
                        file_path.display()
                    ),
                }
            }
        }

        // Wait for command to exit
        match child.wait().await {
            Ok(status) => {
                if !status.success() {
                    tracing::warn!("filename search command exited with status: {}", status);
                }
            }
            Err(e) => {
                tracing::error!("failed to wait for filename search command: {e}");
            }
        }

        // Report completion
        let _ = task_tx.send(TaskResult::ok(
            task_id,
            format!("found {} filename match(es)", results.len()),
        ));

        // Forward results to UI
        let _ = action_tx.send(Action::ShowFilenameSearchResults(results));
    });
}

// ---- helper impls for brevity ---------------------------------------------
trait TaskResultExt {
    fn ok(id: u64, msg: String) -> Self;
    fn error(id: u64, msg: String) -> Self;
}

impl TaskResultExt for TaskResult {
    fn ok(id: u64, msg: String) -> Self {
        Self {
            task_id: id,
            result: Ok(msg),
            progress: Some(1.0),
            current_item: None,
            completed: None,
            total: None,
            message: None,
        }
    }
    fn error(id: u64, msg: String) -> Self {
        Self {
            task_id: id,
            result: Err(msg),
            progress: Some(1.0),
            current_item: None,
            completed: None,
            total: None,
            message: None,
        }
    }
}
