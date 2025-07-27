//! file_ops_task.rs – Updated background file operations
//!
//! This module defines a high‑level `FileOperationTask` that performs copy,
//! move and rename operations asynchronously.  It reports progress and
//! completion through the unified [`TaskResult`] enum defined in the modern
//! event loop.  Unlike the original implementation, this version no longer
//! depends on `AppState` and does not carry a reference to it.  It also
//! produces progress updates using the new `FileOperationProgress` variant,
//! leaving throughput statistics optional.

use crate::controller::event_loop::TaskResult;
use crate::error::AppError;
use std::fmt;
use std::io::{Error, ErrorKind};
use std::path::{Path, PathBuf};
use std::time::Instant;
use tokio::fs as TokioFs;
use tokio::fs::{File, ReadDir};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc::{self, UnboundedSender, error::SendError};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const BUFFER_SIZE: usize = 64 * 1024;

/// File operation task for background processing.
///
/// A `FileOperationTask` encapsulates a single file or directory operation
/// (copy, move or rename).  It reports progress via an unbounded channel of
/// [`TaskResult`].  Cancellation is supported through a [`CancellationToken`].
#[derive(Debug)]
pub struct FileOperationTask {
    /// Unique identifier for this operation.  Generated automatically when
    /// constructing the task.
    pub operation_id: String,
    /// The operation to execute.
    pub operation: FileOperation,
    /// Channel on which progress and completion updates are sent.
    pub task_tx: UnboundedSender<TaskResult>,
    /// Token used to cancel the operation.
    pub cancel_token: CancellationToken,
}

/// Types of file operations supported.
#[derive(Debug, Clone)]
pub enum FileOperation {
    /// Copy a file or directory from `source` to `dest`.  For directories
    /// the copy is performed recursively.
    Copy { source: PathBuf, dest: PathBuf },
    /// Move a file or directory from `source` to `dest`.  On the same
    /// filesystem this is implemented via rename; otherwise a copy and
    /// delete fallback is used.
    Move { source: PathBuf, dest: PathBuf },
    /// Rename a file or directory by changing its filename within the same
    /// parent directory.
    Rename { source: PathBuf, new_name: String },
}

impl fmt::Display for FileOperation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            FileOperation::Copy { .. } => "Copy",
            FileOperation::Move { .. } => "Move",
            FileOperation::Rename { .. } => "Rename",
        };
        write!(f, "{}", s)
    }
}

impl FileOperationTask {
    /// Create a new file operation task with a unique ID.
    pub fn new(
        operation: FileOperation,
        task_tx: UnboundedSender<TaskResult>,
        cancel_token: CancellationToken,
    ) -> Self {
        Self {
            operation_id: Uuid::new_v4().to_string(),
            operation,
            task_tx,
            cancel_token,
        }
    }

    /// Execute the file operation with progress reporting.
    ///
    /// This method calculates the total size and number of files involved
    /// up front, emits an initial progress update, then performs the
    /// operation while periodically reporting incremental progress.  A
    /// completion update is sent at the end regardless of success or
    /// failure.  Errors during execution are returned to the caller.
    pub async fn execute(&self) -> Result<(), AppError> {
        use FileOperation::*;

        // Bail out immediately if cancelled before starting.
        if self.cancel_token.is_cancelled() {
            return Err(Self::error(
                ErrorKind::Interrupted,
                "Operation was cancelled.",
            ));
        }

        // Compute total bytes and number of files for progress reporting.
        let (total_bytes, total_files) = self.calculate_operation_size().await?;
        let mut current_bytes: u64 = 0;
        let mut files_completed: u32 = 0;

        // Determine the initial file for the first progress report.
        let initial_file: &Path = match &self.operation {
            Copy { source, .. } | Move { source, .. } | Rename { source, .. } => source.as_path(),
        };

        // Emit initial progress (0 bytes processed).
        self.report_progress(
            0,
            total_bytes,
            initial_file,
            &mut files_completed,
            total_files,
        )
        .await?;

        // Perform the operation.
        let result: Result<(), AppError> = match &self.operation {
            Copy { source, dest } => {
                self.copy_file_with_progress(
                    source,
                    dest,
                    &mut current_bytes,
                    total_bytes,
                    &mut files_completed,
                    total_files,
                )
                .await
            }
            Move { source, dest } => {
                self.move_file_with_progress(
                    source,
                    dest,
                    &mut current_bytes,
                    total_bytes,
                    &mut files_completed,
                    total_files,
                )
                .await
            }
            Rename { source, new_name } => {
                self.rename_with_progress(
                    source,
                    new_name,
                    &mut current_bytes,
                    total_bytes,
                    &mut files_completed,
                    total_files,
                )
                .await
            }
        };

        // Always emit a completion update after finishing.
        let completion = TaskResult::FileOperationComplete {
            operation_id: self.operation_id.clone(),
            result: result.clone(),
        };
        let _ = self.task_tx.send(completion);

        // Return the result to the caller (for error propagation).
        result
    }

    /// Recursively calculate the size and file count of a directory.
    async fn calculate_directory_size(&self, dir_path: &Path) -> Result<(u64, u32), AppError> {
        let mut total_size: u64 = 0;
        let mut file_count: u32 = 0;
        let mut stack: Vec<PathBuf> = vec![dir_path.to_path_buf()];
        while let Some(current_dir) = stack.pop() {
            let mut entries: ReadDir = TokioFs::read_dir(&current_dir).await?;
            while let Some(entry) = entries.next_entry().await? {
                let path: PathBuf = entry.path();
                if path.is_file() {
                    let metadata = TokioFs::metadata(&path).await?;
                    total_size += metadata.len();
                    file_count += 1;
                } else if path.is_dir() {
                    stack.push(path);
                }
            }
        }
        Ok((total_size, file_count))
    }

    /// Calculate the total size and file count for the operation.
    async fn calculate_operation_size(&self) -> Result<(u64, u32), AppError> {
        match &self.operation {
            FileOperation::Copy { source, .. } | FileOperation::Move { source, .. } => {
                if source.is_file() {
                    let metadata = TokioFs::metadata(source).await?;
                    Ok((metadata.len(), 1))
                } else if source.is_dir() {
                    self.calculate_directory_size(source).await
                } else {
                    Ok((0, 0))
                }
            }
            FileOperation::Rename { source, .. } => {
                // Renaming is constant time; progress is just one file.
                let metadata = TokioFs::metadata(source).await?;
                Ok((metadata.len(), 1))
            }
        }
    }

    /// Report progress to the UI via the task channel.
    async fn report_progress(
        &self,
        current_bytes: u64,
        total_bytes: u64,
        current_file: &Path,
        files_completed: &mut u32,
        total_files: u32,
    ) -> Result<(), AppError> {
        // Build a progress variant.  Throughput is optional and not computed
        // here; callers may choose to compute it externally.
        let progress = TaskResult::FileOperationProgress {
            operation_id: self.operation_id.clone(),
            operation_type: self.operation.to_string(),
            current_bytes,
            total_bytes,
            current_file: current_file.to_path_buf(),
            files_completed: *files_completed,
            total_files,
            start_time: Instant::now(),
            throughput_bps: None,
        };
        self.task_tx
            .send(progress)
            .map_err(|e: SendError<TaskResult>| {
                let err: Error =
                    Error::new(ErrorKind::BrokenPipe, format!("Async send error: {e}"));
                AppError::Io(err)
            })?;
        Ok(())
    }

    /// Copy a file or directory with progress reporting using streaming.
    async fn copy_file_with_progress(
        &self,
        source: &PathBuf,
        dest: &Path,
        current_bytes: &mut u64,
        total_bytes: u64,
        files_completed: &mut u32,
        total_files: u32,
    ) -> Result<(), AppError> {
        // Determine the destination path.  If the destination is a directory
        // then append the source file name; otherwise use dest directly.
        let final_dst: PathBuf = if dest.is_dir() {
            if let Some(filename) = source.file_name() {
                dest.join(filename)
            } else {
                return Err(Self::error(
                    ErrorKind::InvalidInput,
                    "Cannot determine filename from source.",
                ));
            }
        } else {
            dest.to_path_buf()
        };
        // Ensure parent directory exists.
        if let Some(parent) = final_dst.parent() {
            if !parent.exists() {
                TokioFs::create_dir_all(parent).await?;
            }
        }
        // Get the file size for progress tracking.
        let metadata = TokioFs::metadata(source).await?;
        let file_size: u64 = metadata.len();
        // Emit a progress update before starting the copy.
        self.report_progress(
            *current_bytes,
            total_bytes,
            source,
            files_completed,
            total_files,
        )
        .await?;
        // Open files for reading and writing.
        let mut src_file: File = TokioFs::File::open(source).await?;
        let mut dst_file: File = TokioFs::File::create(&final_dst).await?;
        // 64KB buffer for copying.
        let mut buffer: Vec<u8> = vec![0; BUFFER_SIZE];
        let mut copied: u64 = 0;
        // Determine an optimal interval for progress updates (1MB or 10% of file size).
        let kib: u64 = 1024 * 1024;
        let size: u64 = std::cmp::min(kib, file_size / 10);
        let optimal_interval: u64 = std::cmp::max(size, 1);
        // Main copy loop.
        loop {
            let bytes_read: usize = src_file.read(&mut buffer).await?;
            if self.cancel_token.is_cancelled() {
                return Err(Self::error(
                    ErrorKind::Interrupted,
                    "Operation was cancelled.",
                ));
            }
            if bytes_read == 0 {
                break;
            }
            dst_file.write_all(&buffer[..bytes_read]).await?;
            copied += bytes_read as u64;
            *current_bytes += bytes_read as u64;
            // Emit a progress update at defined intervals.
            if copied % optimal_interval == 0 {
                self.report_progress(
                    *current_bytes,
                    total_bytes,
                    source,
                    files_completed,
                    total_files,
                )
                .await?;
            }
        }
        dst_file.flush().await?;
        // Increment file completion count and emit a final progress update for this file.
        *files_completed += 1;
        self.report_progress(
            *current_bytes,
            total_bytes,
            source,
            files_completed,
            total_files,
        )
        .await?;
        Ok(())
    }

    /// Move a file or directory with progress reporting.
    async fn move_file_with_progress(
        &self,
        source: &PathBuf,
        dest: &Path,
        current_bytes: &mut u64,
        total_bytes: u64,
        files_completed: &mut u32,
        total_files: u32,
    ) -> Result<(), AppError> {
        // Determine final destination path.  If dest is a directory, append the file name.
        let final_dst: PathBuf = if dest.is_dir() {
            if let Some(filename) = source.file_name() {
                dest.join(filename)
            } else {
                return Err(Self::error(
                    ErrorKind::InvalidInput,
                    "Cannot determine filename from source for move operation.",
                ));
            }
        } else {
            dest.to_path_buf()
        };
        // Ensure parent directory exists.
        if let Some(parent) = final_dst.parent() {
            if !parent.exists() {
                TokioFs::create_dir_all(parent).await?;
            }
        }
        // Get file size for progress tracking.
        let metadata = TokioFs::metadata(source).await?;
        let file_size: u64 = metadata.len();
        // Emit initial progress.
        self.report_progress(
            *current_bytes,
            total_bytes,
            source,
            files_completed,
            total_files,
        )
        .await?;
        // If cancelled, stop immediately.
        if self.cancel_token.is_cancelled() {
            return Err(Self::error(
                ErrorKind::Interrupted,
                "Operation was cancelled.",
            ));
        }
        // Try rename first (fast path on same filesystem).
        match TokioFs::rename(source, &final_dst).await {
            Ok(()) => {
                *current_bytes += file_size;
                *files_completed += 1;
                self.report_progress(
                    *current_bytes,
                    total_bytes,
                    source,
                    files_completed,
                    total_files,
                )
                .await?;
                Ok(())
            }
            Err(_) => {
                // Fall back to copy then delete.
                self.copy_file_with_progress(
                    source,
                    &final_dst,
                    current_bytes,
                    total_bytes,
                    files_completed,
                    total_files,
                )
                .await?;
                // Remove source after successful copy.
                if source.is_file() {
                    TokioFs::remove_file(source).await?;
                } else if source.is_dir() {
                    TokioFs::remove_dir_all(source).await?;
                }
                Ok(())
            }
        }
    }

    /// Rename a file or directory with progress reporting.
    async fn rename_with_progress(
        &self,
        source: &PathBuf,
        new_name: &str,
        current_bytes: &mut u64,
        total_bytes: u64,
        files_completed: &mut u32,
        total_files: u32,
    ) -> Result<(), AppError> {
        // Ensure the source exists.
        if !source.exists() {
            return Err(AppError::NotFound(source.clone()));
        }
        // Determine the parent directory.
        let parent: &Path = source.parent().ok_or_else(|| AppError::InvalidInput {
            field: "path".into(),
            message: "Cannot rename root directory".into(),
        })?;
        let new_path: PathBuf = parent.join(new_name);
        // Get file size for progress tracking.
        let metadata = TokioFs::metadata(source).await?;
        let file_size: u64 = metadata.len();
        // Emit initial progress.
        self.report_progress(
            *current_bytes,
            total_bytes,
            source,
            files_completed,
            total_files,
        )
        .await?;
        // Check cancellation before starting.
        if self.cancel_token.is_cancelled() {
            return Err(Self::error(
                ErrorKind::Interrupted,
                "Operation was cancelled.",
            ));
        }
        // Perform rename.
        TokioFs::rename(source, &new_path).await?;
        *current_bytes += file_size;
        *files_completed += 1;
        // Emit final progress.
        self.report_progress(
            *current_bytes,
            total_bytes,
            &new_path,
            files_completed,
            total_files,
        )
        .await?;
        Ok(())
    }

    /// Helper to convert an IO error into [`AppError::Io`].
    #[inline(always)]
    fn error(err_kind: ErrorKind, err_msg: &'static str) -> AppError {
        let err: Error = Error::new(err_kind, err_msg);
        AppError::Io(err)
    }
}
