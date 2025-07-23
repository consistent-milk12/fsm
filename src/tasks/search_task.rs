//! src/tasks/search_task.rs
//! ============================================================================
//! # Search Task: Background ripgrep search with raw output
//!
//! Spawns an async ripgrep child process, captures raw output line‑by‑line,
//! and reports the result set back to the UI for direct display without blocking.

use std::process::Stdio;
use std::{path::PathBuf, process::ExitStatus};

use ansi_to_tui::IntoText;
use ratatui::text::Text;
use tokio::{
    io::{AsyncBufReadExt, BufReader, Lines},
    process::{ChildStdout, Command},
    sync::mpsc::UnboundedSender,
};

use crate::controller::{actions::Action, event_loop::TaskResult};

#[derive(Debug, Clone)]
pub struct RawSearchResult {
    pub lines: Vec<String>,
    pub parsed_lines: Vec<Text<'static>>,
    pub total_matches: usize,
    pub base_directory: PathBuf,
}

impl RawSearchResult {
    /// Strip ANSI escape codes from a string
    fn strip_ansi_codes(input: &str) -> String {
        // Simple regex-free approach to strip ANSI codes
        let mut result = String::new();
        let mut chars = input.chars();

        while let Some(c) = chars.next() {
            if c == '\x1b' {
                // Skip ANSI escape sequence
                if chars.next() == Some('[') {
                    // Skip until we find the end character (usually 'm', but could be others)
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

    /// Parse file information from a ripgrep output line
    /// Format: "filename:line_number:content" or just "filename" for headings
    pub fn parse_file_info(line: &str) -> Option<(PathBuf, Option<u32>)> {
        // Strip ANSI color codes first
        let clean_line = Self::strip_ansi_codes(line);

        // Skip empty lines and context separators
        if clean_line.trim().is_empty() || clean_line.starts_with("--") {
            return None;
        }

        // Check if it's a file heading (no line number, just filename)
        if !clean_line.contains(':') {
            return Some((PathBuf::from(clean_line.trim()), None));
        }

        // Parse "filename:line_number:content" format
        let parts: Vec<&str> = clean_line.splitn(3, ':').collect();
        if parts.len() >= 2 {
            let file_path = PathBuf::from(parts[0].trim());
            let line_number = parts[1].trim().parse::<u32>().ok();
            Some((file_path, line_number))
        } else {
            None
        }
    }

    /// Parse file information and resolve relative paths against base directory
    pub fn parse_file_info_with_base(
        line: &str,
        base_dir: &std::path::Path,
    ) -> Option<(PathBuf, Option<u32>)> {
        Self::parse_file_info(line).map(|(path, line_num)| {
            let absolute_path = if path.is_absolute() {
                path
            } else {
                base_dir.join(path)
            };
            (absolute_path, line_num)
        })
    }
}

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

        // Build simple `rg` command with line numbers and color
        let mut child = match Command::new("rg")
            .arg("--line-number")
            .arg("--with-filename")
            .arg("--color=always") // Preserve colors for TUI display
            .arg("--heading") // Group by file
            .arg("--context=1") // Show 1 line before/after for context
            .arg(&pattern)
            .arg(&path)
            .kill_on_drop(true)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                let _ = task_tx.send(TaskResult::error(
                    task_id,
                    format!("failed to spawn ripgrep: {e}"),
                ));
                return;
            }
        };

        // Stream ripgrep stdout line‑by‑line
        let stdout: ChildStdout = child.stdout.take().expect("stdout must be piped");
        let mut reader: Lines<BufReader<ChildStdout>> = BufReader::new(stdout).lines();

        while let Ok(Some(line)) = reader.next_line().await {
            if !line.trim().is_empty() {
                // Store raw line for parsing file info
                output_lines.push(line.clone());

                // Parse ANSI colors and convert to ratatui Text
                match line.as_bytes().to_vec().into_text() {
                    Ok(parsed_text) => {
                        parsed_lines.push(parsed_text);
                    }
                    Err(_) => {
                        // Fallback to plain text if ANSI parsing fails
                        parsed_lines.push(Text::raw(line));
                    }
                }
            }
        }

        // Wait for rg to exit and check status
        let status: ExitStatus = match child.wait().await {
            Ok(status) => status,
            Err(e) => {
                let _ = task_tx.send(TaskResult::error(
                    task_id,
                    format!("failed to wait for ripgrep: {e}"),
                ));
                return;
            }
        };

        if !status.success() && status.code() != Some(1) {
            // Status code 1 means no matches found, which is not an error
            let _ = task_tx.send(TaskResult::error(
                task_id,
                format!("ripgrep failed with status: {status}"),
            ));
            return;
        }

        let match_count: usize = output_lines.len();

        // Report completion to task loop
        let _ = task_tx.send(TaskResult::ok(
            task_id,
            format!("found {match_count} line(s) matching pattern"),
        ));

        // Send raw results to UI
        let raw_result: RawSearchResult = RawSearchResult {
            lines: output_lines,
            parsed_lines,
            total_matches: match_count,
            base_directory: path.clone(),
        };
        let _ = action_tx.send(Action::ShowRawSearchResults(raw_result));
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
