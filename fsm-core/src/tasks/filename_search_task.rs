//! search_task.rs – Background ripgrep search task adapted for the new event loop
//!
//! This task spawns a `ripgrep` process to search for a given pattern in a
//! directory.  It streams the raw output line‑by‑line, collects both the
//! unparsed lines and their ANSI‑colored representation, and sends the
//! final result back to the UI via an [`Action`].  It also reports
//! success or failure through the unified [`TaskResult::Legacy`] variant.

use std::path::PathBuf;
use std::process::ExitStatus;
use std::process::Stdio;

use ansi_to_tui::IntoText;
use ratatui::text::Text;
use tokio::io::{AsyncBufReadExt, BufReader, Lines};
use tokio::process::{ChildStdout, Command};
use tokio::sync::mpsc::UnboundedSender;

use crate::controller::{actions::Action, event_loop::TaskResult};
use crate::error::AppError;

/// Raw search results returned by the search task.
#[derive(Debug, Clone, PartialEq)]
pub struct RawFileSearchResult {
    pub lines: Vec<String>,
    pub parsed_lines: Vec<Text<'static>>,
    pub total_matches: usize,
    pub base_directory: PathBuf,
}

impl RawFileSearchResult {
    /// Strip ANSI escape codes from a string.  This helper is used when
    /// parsing file information from ripgrep output.
    pub fn strip_ansi_codes(input: &str) -> String {
        let mut result = String::new();
        let mut chars = input.chars();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                // Skip ANSI escape sequence
                if chars.next() == Some('[') {
                    // Skip until we find an alphabetic terminator
                    for next_char in chars.by_ref() {
                        if next_char.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
            } else {
                result.push(c);
            }
        }
        result
    }

    /// Parse file information from a ripgrep output line.  Returns the
    /// (path, line number) if the line contains match information, or
    /// `None` otherwise.
    pub fn parse_file_info(line: &str) -> Option<(PathBuf, Option<u32>)> {
        let clean_line = Self::strip_ansi_codes(line);
        if clean_line.trim().is_empty() || clean_line.starts_with("--") {
            return None;
        }
        if !clean_line.contains(':') {
            let path = PathBuf::from(clean_line.trim());
            return Some((path, None));
        }
        let parts: Vec<&str> = clean_line.splitn(3, ':').collect();
        if parts.len() >= 3 {
            let file_path = PathBuf::from(parts[0].trim());
            let line_number = parts[1].trim().parse::<u32>().ok();
            return Some((file_path, line_number));
        } else if parts.len() == 2 {
            if let Ok(_line_num) = parts[0].trim().parse::<u32>() {
                return None;
            } else {
                let file_path = PathBuf::from(parts[0].trim());
                return Some((file_path, None));
            }
        }
        None
    }

    /// Parse file information relative to a base directory, adjusting
    /// relative paths to absolute ones.
    pub fn parse_file_info_with_base(
        line: &str,
        base_dir: &std::path::Path,
    ) -> Option<(PathBuf, Option<u32>)> {
        Self::parse_file_info(line).map(|(path, line_num)| {
            let absolute = if path.is_absolute() {
                path
            } else {
                base_dir.join(path)
            };
            (absolute, line_num)
        })
    }

    /// Parse a line from ripgrep with stateful context when using
    /// `--heading`.  Maintains the current filename between matches.
    pub fn parse_heading_line_with_context(
        line: &str,
        current_file: &mut Option<PathBuf>,
        base_dir: &std::path::Path,
    ) -> Option<(PathBuf, Option<u32>)> {
        let clean_line = Self::strip_ansi_codes(line);
        if clean_line.trim().is_empty() || clean_line.starts_with("--") {
            return None;
        }
        // Filename heading (no colon)
        if !clean_line.contains(':') {
            // Skip context indicator lines like "63-" or "42+"
            if let Some(first_char) = clean_line.chars().next() {
                if first_char.is_ascii_digit() && clean_line.get(1..2) == Some("-")
                    || clean_line.get(1..2) == Some("+")
                {
                    return None;
                }
            }
            let path = base_dir.join(clean_line.trim());
            *current_file = Some(path.clone());
            return Some((path, None));
        }
        // Format: filename:line:content or line:content
        let parts: Vec<&str> = clean_line.splitn(3, ':').collect();
        if parts.len() >= 3 {
            let file_path = PathBuf::from(parts[0].trim());
            let line_number = parts[1].trim().parse::<u32>().ok();
            return Some((file_path, line_number));
        } else if parts.len() == 2 {
            if let Ok(line_num) = parts[0].trim().parse::<u32>() {
                if let Some(current_path) = current_file {
                    return Some((current_path.clone(), Some(line_num)));
                }
                return None;
            } else {
                let file_path = PathBuf::from(parts[0].trim());
                return Some((file_path, None));
            }
        }
        None
    }
}

/// Spawn a background ripgrep search task.  The pattern and path specify
/// what to search.  Results and completion messages are sent back via the
/// provided channels.
pub fn search_task(
    task_id: u64,
    pattern: String,
    path: PathBuf,
    task_tx: UnboundedSender<TaskResult>,
    action_tx: UnboundedSender<Action>,
) {
    tokio::spawn(async move {
        let mut output_lines: Vec<String> = Vec::new();
        let mut parsed_lines: Vec<Text<'static>> = Vec::new();
        // Build the ripgrep command.  We preserve ANSI colors for the TUI and
        // group matches by file using --heading.
        let mut child = match Command::new("rg")
            .arg("--line-number")
            .arg("--with-filename")
            .arg("--color=always")
            .arg("--heading")
            .arg("--context=1")
            .arg(&pattern)
            .arg(&path)
            .kill_on_drop(true)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                let msg = format!("failed to spawn ripgrep: {e}");
                let _ = task_tx.send(TaskResult::Legacy {
                    task_id,
                    result: Err(AppError::Ripgrep(msg.clone())),
                    progress: None,
                    current_item: None,
                    completed: None,
                    total: None,
                    message: Some(msg),
                    execution_time: None,
                    memory_usage: None,
                });
                return;
            }
        };
        // Stream ripgrep stdout line‑by‑line.
        let stdout: ChildStdout = child.stdout.take().expect("stdout must be piped");
        let mut reader: Lines<BufReader<ChildStdout>> = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            if !line.trim().is_empty() {
                output_lines.push(line.clone());
                match line.as_bytes().to_vec().into_text() {
                    Ok(parsed) => parsed_lines.push(parsed),
                    Err(_) => parsed_lines.push(Text::raw(line)),
                }
            }
        }
        // Wait for ripgrep to exit and inspect the status.
        let status: ExitStatus = match child.wait().await {
            Ok(status) => status,
            Err(e) => {
                let msg = format!("failed to wait for ripgrep: {e}");
                let _ = task_tx.send(TaskResult::Legacy {
                    task_id,
                    result: Err(AppError::Ripgrep(msg.clone())),
                    progress: None,
                    current_item: None,
                    completed: None,
                    total: None,
                    message: Some(msg),
                    execution_time: None,
                    memory_usage: None,
                });
                return;
            }
        };
        if !status.success() && status.code() != Some(1) {
            // Exit code 1 means no matches; treat others as errors.
            let msg = format!("ripgrep failed with status: {status}");
            let _ = task_tx.send(TaskResult::Legacy {
                task_id,
                result: Err(AppError::Ripgrep(msg.clone())),
                progress: None,
                current_item: None,
                completed: None,
                total: None,
                message: Some(msg),
                execution_time: None,
                memory_usage: None,
            });
            return;
        }
        let match_count: usize = output_lines.len();
        // Report completion via TaskResult.
        let completion_msg = format!("found {match_count} line(s) matching pattern");
        let _ = task_tx.send(TaskResult::Legacy {
            task_id,
            result: Ok(()),
            progress: Some(match_count as u64),
            current_item: None,
            completed: Some(match_count as u64),
            total: None,
            message: Some(completion_msg.clone()),
            execution_time: None,
            memory_usage: None,
        });
        // Send raw search results to UI via Action.
        let raw_result = RawFileSearchResult {
            lines: output_lines,
            parsed_lines,
            total_matches: match_count,
            base_directory: path.clone(),
        };

        let _ = action_tx.send(Action::ShowRawFileSearchResults(raw_result));
    });
}
