//! Optimized search tasks with proper event loop integration
use anyhow::Result;
use std::path::PathBuf;
use std::process::{ExitStatus, Stdio};
use std::sync::Arc;
use std::time::Instant;
use tokio::io::{AsyncBufReadExt, BufReader, Lines};
use tokio::process::{Child, ChildStdout, Command};
use tokio::sync::mpsc::UnboundedSender;
use tracing::{debug, warn};

use crate::controller::event_loop::TaskResult;
use crate::error::AppError;
use crate::fs::object_info::ObjectInfo;

/// Spawn content search task using ripgrep
pub fn spawn_content_search(
    task_id: u64,
    pattern: String,
    path: PathBuf,
    task_tx: UnboundedSender<TaskResult>,
) {
    tokio::spawn(async move {
        let start_time: Instant = Instant::now();

        match execute_ripgrep_search(&pattern, &path).await {
            Ok(lines) => {
                let task_result: TaskResult = TaskResult::ContentSearchDone {
                    task_id,
                    query: pattern,
                    results: lines,
                    exec: start_time.elapsed(),
                };

                if let Err(e) = task_tx.send(task_result) {
                    warn!("Failed to send search results: {}", e);
                }
            }

            Err(e) => {
                let task_result: TaskResult = TaskResult::Generic {
                    task_id,
                    result: Err(Arc::new(AppError::Ripgrep(e.to_string()))),
                    msg: Some(format!("Content search failed: {e}")),
                    exec: start_time.elapsed(),
                };

                if let Err(e) = task_tx.send(task_result) {
                    warn!("Failed to send search error: {}", e);
                }
            }
        }
    });
}

/// Spawn filename search task
pub fn spawn_filename_search(
    task_id: u64,
    pattern: String,
    path: PathBuf,
    task_tx: UnboundedSender<TaskResult>,
) {
    tokio::spawn(async move {
        let start_time: Instant = Instant::now();

        match execute_filename_search(&pattern, &path).await {
            Ok(object_infos) => {
                let task_result: TaskResult = TaskResult::SearchDone {
                    task_id,
                    query: pattern,
                    results: object_infos,
                    exec: start_time.elapsed(),
                };

                if let Err(e) = task_tx.send(task_result) {
                    warn!("Failed to send filename search results: {}", e);
                }
            }

            Err(e) => {
                let task_result: TaskResult = TaskResult::Generic {
                    task_id,
                    result: Err(Arc::new(AppError::Ripgrep(e.to_string()))),
                    msg: Some(format!("Filename search failed: {e}")),
                    exec: start_time.elapsed(),
                };

                if let Err(e) = task_tx.send(task_result) {
                    warn!("Failed to send filename search error: {}", e);
                }
            }
        }
    });
}

async fn execute_ripgrep_search(pattern: &str, path: &PathBuf) -> Result<Vec<String>> {
    debug!(
        "Starting ripgrep search for '{}' in {}",
        pattern,
        path.display()
    );

    let mut child = Command::new("rg")
        .arg("--line-number")
        .arg("--with-filename")
        .arg("--color=always")
        .arg("--heading")
        .arg("--context=1")
        .arg(pattern)
        .arg(path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let stdout: ChildStdout = child.stdout.take().unwrap();
    let mut reader: Lines<BufReader<ChildStdout>> = BufReader::new(stdout).lines();

    let mut lines: Vec<String> = Vec::new();

    while let Ok(Some(line)) = reader.next_line().await {
        if !line.trim().is_empty() {
            lines.push(line);
        }
    }

    let status: ExitStatus = child.wait().await?;

    if !status.success() && status.code() != Some(1) {
        return Err(anyhow::anyhow!("ripgrep failed with status: {}", status));
    }

    Ok(lines)
}

async fn execute_filename_search(pattern: &str, path: &PathBuf) -> Result<Vec<ObjectInfo>> {
    debug!(
        "Starting filename search for '{}' in {}",
        pattern,
        path.display()
    );

    let mut child: Child = Command::new("find")
        .arg(path)
        .arg("-name")
        .arg(format!("*{pattern}*"))
        .arg("-type")
        .arg("f")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    let stdout: ChildStdout = child.stdout.take().unwrap();
    let mut reader: Lines<BufReader<ChildStdout>> = BufReader::new(stdout).lines();

    let mut results: Vec<ObjectInfo> = Vec::new();

    while let Ok(Some(line)) = reader.next_line().await {
        if !line.trim().is_empty() {
            let path: PathBuf = PathBuf::from(line);

            if let Ok(light_info) = ObjectInfo::from_path_light(&path).await {
                results.push(ObjectInfo::with_placeholder_metadata(light_info));
            }
        }
    }

    let status: ExitStatus = child.wait().await?;

    if !status.success() {
        return Err(anyhow::anyhow!("find failed with status: {}", status));
    }

    Ok(results)
}
