//! Optimized search tasks with proper event loop integration

use ansi_to_tui::IntoText;
use anyhow::Result;
use ratatui::text::Text;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{debug, warn};

use crate::controller::event_loop::TaskResult;
use crate::error::AppError;

#[derive(Debug, Clone)]
pub struct SearchResults {
    pub lines: Vec<String>,
    pub parsed_lines: Vec<Text<'static>>,
    pub total_matches: usize,
    pub base_directory: PathBuf,
    pub exec: Duration,
}

/// Spawn content search task using ripgrep
pub fn spawn_content_search(
    task_id: u64,
    pattern: String,
    path: PathBuf,
    task_tx: UnboundedSender<TaskResult>,
) {
    tokio::spawn(async move {
        let start_time = Instant::now();

        match execute_ripgrep_search(&pattern, &path).await {
            Ok(results) => {
                let task_result = TaskResult::ContentSearchDone {
                    task_id,
                    query: pattern,
                    results: results.lines,
                    exec: start_time.elapsed(),
                };

                if let Err(e) = task_tx.send(task_result) {
                    warn!("Failed to send search results: {}", e);
                }
            }
            Err(e) => {
                let task_result = TaskResult::Generic {
                    task_id,
                    result: Err(AppError::Ripgrep(e.to_string())),
                    msg: Some(format!("Search failed: {}", e)),
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
        let start_time = Instant::now();

        match execute_filename_search(&pattern, &path).await {
            Ok(results) => {
                let object_infos = convert_to_object_infos(results.lines, &path).await;

                let task_result = TaskResult::SearchDone {
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
                let task_result = TaskResult::Generic {
                    task_id,
                    result: Err(AppError::Ripgrep(e.to_string())),
                    msg: Some(format!("Filename search failed: {}", e)),
                    exec: start_time.elapsed(),
                };

                if let Err(e) = task_tx.send(task_result) {
                    warn!("Failed to send filename search error: {}", e);
                }
            }
        }
    });
}

async fn execute_ripgrep_search(pattern: &str, path: &PathBuf) -> Result<SearchResults> {
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
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout).lines();

    let mut lines = Vec::new();
    let mut parsed_lines = Vec::new();

    while let Ok(Some(line)) = reader.next_line().await {
        if !line.trim().is_empty() {
            lines.push(line.clone());

            match line.as_bytes().to_vec().into_text() {
                Ok(parsed) => parsed_lines.push(parsed),
                Err(_) => parsed_lines.push(Text::raw(line)),
            }
        }
    }

    let status = child.wait().await?;

    if !status.success() && status.code() != Some(1) {
        return Err(anyhow::anyhow!("ripgrep failed with status: {}", status));
    }

    Ok(SearchResults {
        total_matches: lines.len(),
        lines,
        parsed_lines,
        base_directory: path.clone(),
        exec: Duration::ZERO, // Set by caller
    })
}

async fn execute_filename_search(pattern: &str, path: &PathBuf) -> Result<SearchResults> {
    debug!(
        "Starting filename search for '{}' in {}",
        pattern,
        path.display()
    );

    let mut child = Command::new("find")
        .arg(path)
        .arg("-name")
        .arg(&format!("*{}*", pattern))
        .arg("-type")
        .arg("f")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout).lines();

    let mut lines = Vec::new();

    while let Ok(Some(line)) = reader.next_line().await {
        if !line.trim().is_empty() {
            lines.push(line);
        }
    }

    let status = child.wait().await?;

    if !status.success() {
        return Err(anyhow::anyhow!("find failed with status: {}", status));
    }

    Ok(SearchResults {
        total_matches: lines.len(),
        parsed_lines: lines.iter().map(|l| Text::raw(l.clone())).collect(),
        lines,
        base_directory: path.clone(),
        exec: Duration::ZERO,
    })
}

async fn convert_to_object_infos(
    file_paths: Vec<String>,
    _base_dir: &PathBuf,
) -> Vec<crate::fs::object_info::ObjectInfo> {
    let mut results = Vec::new();

    for path_str in file_paths {
        let path = PathBuf::from(path_str);

        if let Ok(light_info) = crate::fs::object_info::ObjectInfo::from_path_light(&path).await {
            results.push(crate::fs::object_info::ObjectInfo::with_placeholder_metadata(light_info));
        }
    }

    results
}
