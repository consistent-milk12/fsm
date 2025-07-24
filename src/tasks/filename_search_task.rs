//! src/tasks/filename_search_task.rs
//! ============================================================================
//! # Enhanced Filename Search Task: Background recursive filename search
//!
//! Spawns an async find/fd process to recursively search for filenames,
//! converts each hit into `ObjectInfo`, and reports results back to the UI.
//!
//! ## Enhancements:
//! - Extensive logging for performance monitoring and debugging
//! - Progress reporting with streaming updates
//! - Improved error handling and recovery
//! - Command selection with fallback strategy
//! - Performance metrics and optimization

use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};

use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::mpsc::UnboundedSender,
};
use tracing::{debug, info, trace, warn};

use crate::{
    controller::{actions::Action, event_loop::TaskResult},
    fs::object_info::ObjectInfo,
};

/// Enhanced filename search task with comprehensive logging and performance monitoring
pub fn filename_search_task(
    task_id: u64,
    pattern: String,
    search_path: PathBuf,
    task_tx: UnboundedSender<TaskResult>,
    action_tx: UnboundedSender<Action>,
) {
    tokio::spawn(async move {
        let task_start = Instant::now();
        let mut results = Vec::<ObjectInfo>::new();
        let mut processed_count = 0usize;
        let mut error_count = 0usize;

        info!(
            "Starting filename search task {}: pattern='{}', path='{}'",
            task_id,
            pattern,
            search_path.display()
        );

        // Validate inputs
        if pattern.trim().is_empty() {
            info!("Filename search task {} received empty pattern", task_id);
            let _ = task_tx.send(TaskResult::error(
                task_id,
                "Search pattern cannot be empty".to_string(),
            ));
            return;
        }

        if !search_path.exists() {
            warn!("Search path does not exist: {}", search_path.display());
            let _ = task_tx.send(TaskResult::error(
                task_id,
                format!("Search path does not exist: {}", search_path.display()),
            ));
            return;
        }

        // Enhanced command selection with detailed logging and command validation
        let (mut child, command_used) = {
            // First check if 'fd' is available by testing it
            debug!("Checking if 'fd' command is available");
            let fd_available = match Command::new("fd")
                .arg("--version")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await
            {
                Ok(status) => status.success(),
                Err(e) => {
                    debug!("'fd' command not available: {}", e);
                    false
                }
            };

            if fd_available {
                debug!("'fd' command found, attempting to use it for search");
                let fd_cmd = Command::new("fd")
                    .arg("--type")
                    .arg("f") // files
                    .arg("--type")
                    .arg("d") // directories
                    .arg("--hidden") // include hidden files
                    .arg("--follow") // follow symlinks
                    .arg("--case-sensitive") // case-sensitive search
                    .arg(&pattern)
                    .arg(&search_path)
                    .kill_on_drop(true)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn();

                match fd_cmd {
                    Ok(child) => {
                        info!(
                            "Successfully spawned 'fd' command for search: fd --type f --type d --hidden --follow --case-sensitive '{}' '{}'",
                            pattern,
                            search_path.display()
                        );
                        (child, "fd")
                    }
                    Err(e) => {
                        info!("'fd' command available but failed to spawn: {}", e);
                        debug!("Falling back to 'find' command");

                        // Fallback to find with corrected syntax
                        let find_cmd = Command::new("find")
                            .arg(&search_path)
                            .arg("(") // Start grouping
                            .arg("-type")
                            .arg("f") // files
                            .arg("-o") // OR
                            .arg("-type")
                            .arg("d") // directories
                            .arg(")") // End grouping
                            .arg("-iname") // case-insensitive name matching
                            .arg(format!("*{pattern}*"))
                            .kill_on_drop(true)
                            .stdout(Stdio::piped())
                            .stderr(Stdio::piped())
                            .spawn();

                        match find_cmd {
                            Ok(c) => {
                                info!(
                                    "Using 'find' command as fallback: find '{}' \\( -type f -o -type d \\) -iname '*{}*'",
                                    search_path.display(),
                                    pattern
                                );
                                (c, "find")
                            }
                            Err(e) => {
                                warn!(
                                    "Both 'fd' and 'find' commands failed to spawn: fd_err=failed_to_spawn, find_err={}",
                                    e
                                );
                                let _ = task_tx.send(TaskResult::error(
                                    task_id,
                                    format!("No suitable search command available: fd failed to spawn, find error: {e}"),
                                ));
                                return;
                            }
                        }
                    }
                }
            } else {
                debug!("'fd' command not found, checking 'find' availability");

                // Check if find is available
                let find_available = match Command::new("find")
                    .arg("--version")
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .await
                {
                    Ok(status) => status.success(),
                    Err(e) => {
                        debug!("'find' command not available: {}", e);
                        false
                    }
                };

                if !find_available {
                    warn!("Neither 'fd' nor 'find' commands are available on this system");
                    let _ = task_tx.send(TaskResult::error(
                        task_id,
                        "Neither 'fd' nor 'find' commands are available on this system".to_string(),
                    ));
                    return;
                }

                debug!("Using 'find' command (primary choice since 'fd' not available)");
                // Use find with corrected syntax - CRITICAL FIX
                let find_cmd = Command::new("find")
                    .arg(&search_path)
                    .arg("(") // Start grouping - FIXED SYNTAX
                    .arg("-type")
                    .arg("f") // files
                    .arg("-o") // OR
                    .arg("-type")
                    .arg("d") // directories
                    .arg(")") // End grouping - FIXED SYNTAX
                    .arg("-iname") // case-insensitive name matching
                    .arg(format!("*{pattern}*"))
                    .kill_on_drop(true)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn();

                match find_cmd {
                    Ok(c) => {
                        info!(
                            "Successfully spawned 'find' command: find '{}' \\( -type f -o -type d \\) -iname '*{}*'",
                            search_path.display(),
                            pattern
                        );
                        (c, "find")
                    }
                    Err(e) => {
                        warn!("Failed to spawn 'find' command: {}", e);
                        let _ = task_tx.send(TaskResult::error(
                            task_id,
                            format!("Find command failed to spawn: {e}"),
                        ));
                        return;
                    }
                }
            }
        };

        debug!("Search command '{}' spawned successfully", command_used);

        // Enhanced streaming with progress reporting and error handling
        let stdout = child.stdout.take().expect("stdout must be piped");
        let stderr = child.stderr.take().expect("stderr must be piped");
        let mut stdout_reader = BufReader::new(stdout).lines();
        let mut stderr_reader = BufReader::new(stderr).lines();

        // Spawn a task to handle stderr output
        let task_id_stderr = task_id;
        tokio::spawn(async move {
            while let Ok(Some(error_line)) = stderr_reader.next_line().await {
                if !error_line.trim().is_empty() {
                    info!("Search command stderr: {}", error_line.trim());
                }
            }
            trace!("Search task {} stderr monitoring complete", task_id_stderr);
        });

        let mut last_progress_report = Instant::now();
        const PROGRESS_REPORT_INTERVAL: Duration = Duration::from_millis(500);

        info!("Starting to process search results from command output");
        let mut line_count = 0;

        while let Ok(Some(line)) = stdout_reader.next_line().await {
            line_count += 1;
            let line = line.trim();

            if line.is_empty() {
                trace!("Skipping empty line #{}", line_count);
                continue;
            }

            processed_count += 1;
            let file_path = PathBuf::from(line);

            if processed_count <= 5 {
                // Log first few results in detail for debugging
                debug!(
                    "Processing search result #{}: '{}' -> path: '{}'",
                    processed_count,
                    line,
                    file_path.display()
                );
            } else if processed_count.is_multiple_of(50) {
                // Log every 50th result to track progress
                debug!(
                    "Processed {} results so far, latest: {}",
                    processed_count,
                    file_path.display()
                );
            } else {
                trace!(
                    "Processing search result #{}: {}",
                    processed_count,
                    file_path.display()
                );
            }

            // Enhanced path validation with detailed logging
            if !file_path.exists() {
                info!(
                    "Search result path does not exist (possibly deleted during search): {}",
                    file_path.display()
                );
                continue;
            }

            // Additional validation
            if !file_path.is_absolute() {
                info!(
                    "Search result is not absolute path: {}",
                    file_path.display()
                );
                // Try to make it absolute relative to search_path
                let absolute_path = search_path.join(&file_path);
                if absolute_path.exists() {
                    debug!(
                        "Converted relative path to absolute: {} -> {}",
                        file_path.display(),
                        absolute_path.display()
                    );
                    // Continue with absolute_path instead of file_path
                } else {
                    info!("Could not resolve relative path: {}", file_path.display());
                    continue;
                }
            }

            // Create ObjectInfo with enhanced error handling and logging
            match ObjectInfo::from_path(&file_path).await {
                Ok(info) => {
                    if processed_count <= 5 {
                        debug!(
                            "Successfully created ObjectInfo for {}: is_dir={}, size={}, name='{}'",
                            file_path.display(),
                            info.is_dir,
                            info.size,
                            info.name
                        );
                    } else {
                        trace!("Successfully added result: {}", file_path.display());
                    }
                    results.push(info);
                }
                Err(e) => {
                    error_count += 1;
                    warn!(
                        "Failed to create ObjectInfo for {}: {} (error #{}/{}, line #{})",
                        file_path.display(),
                        e,
                        error_count,
                        processed_count,
                        line_count
                    );

                    // Log additional context for first few errors
                    if error_count <= 3 {
                        debug!(
                            "Error context: path exists={}, is_absolute={}, parent_exists={}",
                            file_path.exists(),
                            file_path.is_absolute(),
                            file_path.parent().map(|p| p.exists()).unwrap_or(false)
                        );
                    }
                }
            }

            // Send periodic progress updates
            if last_progress_report.elapsed() >= PROGRESS_REPORT_INTERVAL {
                let progress_msg = format!(
                    "Found {} matches (processed {}, {} errors)",
                    results.len(),
                    processed_count,
                    error_count
                );

                debug!("Progress update: {}", progress_msg);

                let _ = task_tx.send(TaskResult::Legacy {
                    task_id,
                    result: Ok(progress_msg.clone()),
                    progress: None, // Indeterminate progress
                    current_item: Some(file_path.to_string_lossy().to_string()),
                    completed: Some(results.len() as u64),
                    total: None,
                    message: Some(progress_msg),
                    execution_time: None, // No timing for progress updates
                    memory_usage: None,   // No memory tracking for progress updates
                });

                // Send intermediate results to UI for responsiveness
                if !results.is_empty() {
                    trace!("Sending intermediate results: {} items", results.len());
                    let _ = action_tx.send(Action::ShowFilenameSearchResults(results.clone()));
                }

                last_progress_report = Instant::now();
            }
        }

        info!(
            "Finished processing search output: {} valid results from {} processed entries ({} errors, {} total lines)",
            results.len(),
            processed_count,
            error_count,
            line_count
        );

        // Log summary of results for debugging
        if results.is_empty() {
            info!("Search completed but no valid results found. This could indicate:");
            info!("  1. No files/directories match the pattern '{}'", pattern);
            info!("  2. Search command failed to find anything");
            info!("  3. All found paths were invalid/inaccessible");
            info!("  4. Command syntax issues (check stderr output above)");
        } else {
            debug!("Search results summary:");
            let dirs = results.iter().filter(|r| r.is_dir).count();
            let files = results.iter().filter(|r| !r.is_dir).count();
            debug!("  - {} directories, {} files", dirs, files);

            // Log first few results for verification
            for (i, result) in results.iter().take(3).enumerate() {
                debug!(
                    "  Result #{}: {} ({})",
                    i + 1,
                    result.name,
                    if result.is_dir { "directory" } else { "file" }
                );
            }
            if results.len() > 3 {
                debug!("  ... and {} more results", results.len() - 3);
            }
        }

        // Enhanced command completion handling
        let wait_start = Instant::now();
        match child.wait().await {
            Ok(status) => {
                let wait_duration = wait_start.elapsed();
                if status.success() {
                    info!(
                        "Search command '{}' completed successfully in {:?} (exit code: {})",
                        command_used,
                        wait_duration,
                        status.code().unwrap_or(-1)
                    );
                } else {
                    info!(
                        "Search command '{}' exited with non-zero status: {} (duration: {:?})",
                        command_used, status, wait_duration
                    );
                }
            }
            Err(e) => {
                warn!(
                    "Failed to wait for search command '{}': {} (after {:?})",
                    command_used,
                    e,
                    wait_start.elapsed()
                );
            }
        }

        // Enhanced completion reporting with performance metrics
        let total_duration = task_start.elapsed();
        let results_count = results.len();

        let completion_message = format!(
            "Found {results_count} filename match(es) in {total_duration:?} (processed {processed_count} entries, {error_count} errors)"
        );

        info!(
            "Filename search task {} completed: {}",
            task_id, completion_message
        );

        // Performance analysis
        if total_duration > Duration::from_secs(5) {
            info!(
                "Slow filename search detected: {} took {:?} for {} results (>{} entries processed)",
                task_id, total_duration, results_count, processed_count
            );
        } else if total_duration > Duration::from_secs(1) {
            debug!(
                "Moderate search time: {} took {:?} for {} results",
                task_id, total_duration, results_count
            );
        } else {
            trace!(
                "Fast search completed: {} took {:?} for {} results",
                task_id, total_duration, results_count
            );
        }

        // Report final completion
        let _ = task_tx.send(TaskResult::ok(task_id, completion_message.clone()));

        // Forward final results to UI
        info!(
            "Sending final search results to UI: {} items",
            results_count
        );
        let _ = action_tx.send(Action::ShowFilenameSearchResults(results));

        trace!("Filename search task {} fully complete", task_id);
    });

    debug!("Filename search task {} spawned successfully", task_id);
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
