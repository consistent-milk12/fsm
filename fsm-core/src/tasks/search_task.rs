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

#[derive(Debug, Clone, PartialEq)]
pub struct RawSearchResult {
    pub lines: Vec<String>,
    pub parsed_lines: Vec<Text<'static>>,
    pub total_matches: usize,
    pub base_directory: PathBuf,
}

impl RawSearchResult {
    /// Strip ANSI escape codes from a string
    pub fn strip_ansi_codes(input: &str) -> String {
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
    /// NOTE: This function should only be used with complete "filename:line:content" lines
    /// For parsing individual lines from ripgrep output, use stateful parsing in the search task
    pub fn parse_file_info(line: &str) -> Option<(PathBuf, Option<u32>)> {
        // Strip ANSI color codes first
        let clean_line = Self::strip_ansi_codes(line);
        tracing::debug!("PARSE: Original line: '{}'", line);
        tracing::debug!("PARSE: Clean line: '{}'", clean_line);

        // Skip empty lines and context separators
        if clean_line.trim().is_empty() || clean_line.starts_with("--") {
            tracing::debug!("PARSE: Skipping empty/separator line");
            return None;
        }

        // Check if it's a file heading (no line number, just filename)
        if !clean_line.contains(':') {
            let path = PathBuf::from(clean_line.trim());
            tracing::debug!("PARSE: File heading detected: {:?}", path);
            return Some((path, None));
        }

        // Parse "filename:line_number:content" format
        let parts: Vec<&str> = clean_line.splitn(3, ':').collect();
        tracing::debug!("PARSE: Split into {} parts: {:?}", parts.len(), parts);

        if parts.len() >= 3 {
            // This should be a complete filename:line:content format
            let file_path = PathBuf::from(parts[0].trim());
            let line_number = parts[1].trim().parse::<u32>().ok();
            tracing::debug!(
                "PARSE: Parsed complete line - file: {:?}, line: {:?}",
                file_path,
                line_number
            );
            Some((file_path, line_number))
        } else if parts.len() == 2 {
            // This might be "line_number:content" format - we need context
            if let Ok(line_num) = parts[0].trim().parse::<u32>() {
                tracing::debug!(
                    "PARSE: Found line:content format without filename - line: {}",
                    line_num
                );
                // Return None because we need filename context
                None
            } else {
                // This might be "filename:something"
                let file_path = PathBuf::from(parts[0].trim());
                tracing::debug!("PARSE: Parsed partial - file: {:?}", file_path);
                Some((file_path, None))
            }
        } else {
            tracing::debug!("PARSE: Failed to parse - insufficient parts");
            None
        }
    }

    /// Parse file information and resolve relative paths against base directory
    /// This function handles stateful parsing for ripgrep --heading format
    pub fn parse_file_info_with_base(
        line: &str,
        base_dir: &std::path::Path,
    ) -> Option<(PathBuf, Option<u32>)> {
        tracing::debug!(
            "PARSE_WITH_BASE: Input line: '{}', base_dir: {:?}",
            line,
            base_dir
        );

        Self::parse_file_info(line).map(|(path, line_num)| {
            tracing::debug!(
                "PARSE_WITH_BASE: Initial parsed path: {:?}, is_absolute: {}",
                path,
                path.is_absolute()
            );

            let absolute_path = if path.is_absolute() {
                tracing::debug!("PARSE_WITH_BASE: Path is already absolute");
                path
            } else {
                let joined = base_dir.join(path);
                tracing::debug!("PARSE_WITH_BASE: Joined relative path: {:?}", joined);
                joined
            };

            tracing::debug!(
                "PARSE_WITH_BASE: Final result - path: {:?}, line: {:?}",
                absolute_path,
                line_num
            );
            (absolute_path, line_num)
        })
    }

    /// Parse a single line from ripgrep --heading output with stateful context
    /// Returns (file_path, line_number) if this line represents a match
    pub fn parse_heading_line_with_context(
        line: &str,
        current_file: &mut Option<PathBuf>,
        base_dir: &std::path::Path,
    ) -> Option<(PathBuf, Option<u32>)> {
        let clean_line = Self::strip_ansi_codes(line);
        tracing::debug!(
            "PARSE_HEADING: Processing line: '{}' with current_file: {:?}",
            clean_line,
            current_file
        );

        // Skip empty lines and context separators
        if clean_line.trim().is_empty() || clean_line.starts_with("--") {
            return None;
        }

        // Check if this is a file heading (no colon, just a filename)
        // But exclude ripgrep context lines that start with line numbers followed by - or +
        if !clean_line.contains(':') {
            // Skip ripgrep context indicators (e.g., "63-", "42+", etc.)
            if let Some(first_char) = clean_line.chars().next()
                && first_char.is_ascii_digit()
            {
                // Look for pattern like "123-" or "123+" which are context lines
                let chars = clean_line.chars();
                let mut found_digits = false;

                for c in chars {
                    if c.is_ascii_digit() {
                        found_digits = true;
                    } else if found_digits && (c == '-' || c == '+') {
                        // This is a context line, not a filename
                        tracing::debug!("PARSE_HEADING: Skipping context line: '{}'", clean_line);
                        return None;
                    } else {
                        break;
                    }
                }
            }

            let path = PathBuf::from(clean_line.trim());
            let absolute_path = if path.is_absolute() {
                path.clone()
            } else {
                base_dir.join(&path)
            };
            *current_file = Some(absolute_path.clone());
            tracing::debug!("PARSE_HEADING: New file heading: {:?}", absolute_path);
            return Some((absolute_path, None));
        }

        // This should be a line:content format
        let parts: Vec<&str> = clean_line.splitn(2, ':').collect();
        if parts.len() == 2
            && let Ok(line_num) = parts[0].trim().parse::<u32>()
        {
            // This is line_number:content format
            if let Some(current_path) = current_file {
                tracing::debug!(
                    "PARSE_HEADING: Found match - file: {:?}, line: {}",
                    current_path,
                    line_num
                );
                return Some((current_path.clone(), Some(line_num)));
            }
            tracing::debug!("PARSE_HEADING: Found line:content but no current file context");
        }

        None
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
        Self::Legacy {
            task_id: id,
            result: Ok(msg),
            progress: Some(1.0),
            current_item: None,
            completed: None,
            total: None,
            message: None,
            execution_time: None, // No execution time tracking in helper methods
            memory_usage: None,   // No memory usage tracking in helper methods
        }
    }
    fn error(id: u64, msg: String) -> Self {
        Self::Legacy {
            task_id: id,
            result: Err(msg),
            progress: Some(1.0),
            current_item: None,
            completed: None,
            total: None,
            message: None,
            execution_time: None, // No execution time tracking in helper methods
            memory_usage: None,   // No memory usage tracking in helper methods
        }
    }
}
