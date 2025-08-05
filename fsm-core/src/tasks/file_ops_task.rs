//! ``src/tasks/file_ops_task.rs``
//! ============================================================================
//! # File Operations Task: Background file operations with progress tracking
//!
//! Handles copy, move, and rename operations asynchronously to prevent UI
//! blocking during large file operations.

use crate::{config::Config, error::AppError, logging_opt::ProfilingData};
use crate::{AppState, controller::event_loop::TaskResult};
use std::{sync::Arc, time::Duration};
use std::{
    fs::Metadata,
    io::{Error, ErrorKind},
    path::{Path, PathBuf},
    time::Instant,
};
use tokio::sync::MutexGuard;
use tokio::{
    fs::{File, ReadDir},
    sync::{
        Mutex,
        mpsc::{self, error::SendError},
    },
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use tokio::fs as TokioFs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const BUFFER_SIZE: usize = 64 * 1024;

/// File operation task for background processing
#[derive(Debug)]
pub struct FileOperationTask {
    pub operation_id: String,
    pub operation: FileOperation,
    pub task_tx: mpsc::UnboundedSender<TaskResult>,
    pub cancel_token: CancellationToken,
    pub app: Arc<Mutex<AppState>>,
}

/// Types of file operations supported
#[derive(Debug, Clone)]
pub enum FileOperation {
    /// Copy file/directory from source to destination
    Copy { source: PathBuf, dest: PathBuf },
    /// Move file/directory from source to destination
    Move { source: PathBuf, dest: PathBuf },
    /// Rename file/directory
    Rename { source: PathBuf, new_name: String },
}

impl std::fmt::Display for FileOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use super::file_ops_task::FileOperation::{Copy, Move, Rename};

        let ret_str: &'static str = match *self {
            Copy { source: _, dest: _ } => "Copy",

            Move { source: _, dest: _ } => "Move",

            Rename {
                source: _,
                new_name: _,
            } => "Rename",
        };

        write!(f, "{ret_str}")
    }
}

impl FileOperationTask {
    /// Create new file operation task with unique ID
    pub fn new(
        operation: FileOperation,
        task_tx: mpsc::UnboundedSender<TaskResult>,
        cancel_token: CancellationToken,
        app: Arc<Mutex<AppState>>,
    ) -> Self {
        Self {
            operation_id: Uuid::new_v4().to_string(),
            operation,
            task_tx,
            cancel_token,
            app,
        }
    }

    /// Execute file operation with full progress reporting
    pub async fn execute(&self) -> Result<(), AppError> {
        use FileOperation::{Copy, Move, Rename};

        let start_time = Instant::now();
        let start_memory_kb = ProfilingData::get_current_memory_kb();

        // Check for cancellation before starting
        if self.cancel_token.is_cancelled() {
            let err_kind: ErrorKind = ErrorKind::Interrupted;
            let err_msg: &'static str = "Operation was cancelled.";

            return Err(Self::error(err_kind, err_msg));
        }

        // Calculate total operation size first
        let (total_bytes, total_files) = self.calculate_operation_size().await?;
        let mut current_bytes: u64 = 0;
        let mut files_completed: u32 = 0;

        // Report initial progress
        let initial_file: &Path = match &self.operation {
            Copy { source, dest: _ }
            | Move { source, dest: _ }
            | Rename {
                source,
                new_name: _,
            } => source,
        };

        self.report_progress(
            0,
            total_bytes,
            initial_file,
            &files_completed,
            total_files,
        )
        .await?;

        // Execute operation with progress tracking
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

        // Calculate final profiling data using new API
        let duration: Duration = start_time.elapsed();
        let config: Config = Config::load().await.unwrap_or_default();
        let _profiling_data: ProfilingData = ProfilingData::collect_profiling_data_conditional(
            start_memory_kb,
            duration,
            &config.profiling
        );

        // Send completion result regardless of success/failure
        let completion_result: TaskResult = TaskResult::FileOperationComplete {
            operation_id: self.operation_id.clone(),
            result: result.clone(),
        };

        let _send_result: Result<(), SendError<TaskResult>> = self.task_tx.send(completion_result);

        // Cleanup operation from UI state
        {
            let mut app: MutexGuard<'_, AppState> = self.app.lock().await;
            app.ui.remove_operation(&self.operation_id);
        }

        result
    }

    /// Recursively calculate directory size and file count
    async fn calculate_directory_size(&self, dir_path: &Path) -> Result<(u64, u32), AppError> {
        let mut total_size: u64 = 0;
        let mut file_count: u32 = 0;

        let mut stack: Vec<PathBuf> = vec![dir_path.to_path_buf()];

        while let Some(current_dir) = stack.pop() {
            let mut entries: ReadDir = TokioFs::read_dir(&current_dir).await?;

            while let Some(entry) = entries.next_entry().await? {
                let path: PathBuf = entry.path();

                if path.is_file() {
                    let metadata: Metadata = TokioFs::metadata(&path).await?;
                    total_size += metadata.len();
                    file_count += 1;
                } else if path.is_dir() {
                    stack.push(path);
                }
            }
        }

        Ok((total_size, file_count))
    }

    /// Calculate total size and file count for progress tracking
    async fn calculate_operation_size(&self) -> Result<(u64, u32), AppError> {
        match &self.operation {
            FileOperation::Copy { source, dest: _ } | FileOperation::Move { source, dest: _ } => {
                if source.is_file() {
                    let metadata: Metadata = TokioFs::metadata(source).await?;

                    Ok((metadata.len(), 1))
                } else if source.is_dir() {
                    self.calculate_directory_size(source).await
                } else {
                    Ok((0, 0))
                }
            }

            FileOperation::Rename {
                source,
                new_name: _,
            } => {
                // Rename is O(1), no progress tracker is needed.
                let metadata: Metadata = TokioFs::metadata(source).await?;

                Ok((metadata.len(), 1))
            }
        }
    }

    #[allow(clippy::unused_async)]
    /// Report progress to UI
    async fn report_progress(
        &self,
        current_bytes: u64,
        total_bytes: u64,
        current_file: &Path,
        files_completed: &u32,
        total_files: u32,
    ) -> Result<(), AppError> {
        let throughput = if current_bytes > 0 {
            Some(current_bytes)
        } else {
            None
        };

        let progress_result: TaskResult = TaskResult::FileOperationProgress {
            operation_id: self.operation_id.clone(),
            operation_type: self.operation.to_string(),
            current_bytes,
            total_bytes,
            current_file: current_file.to_path_buf(),
            files_completed: *files_completed,
            total_files,
            start_time: Instant::now(),
            throughput_bps: throughput,
        };

        self.task_tx
            .send(progress_result)
            .map_err(|e: SendError<TaskResult>| {
                Error::new(ErrorKind::BrokenPipe, format!("Async send error: {e}"))
            })?;

        Ok(())
    }

    /// Copy file with progress reporting using streaming
    async fn copy_file_with_progress(
        &self,
        source: &PathBuf,
        dest: &Path,
        current_bytes: &mut u64,
        total_bytes: u64,
        files_completed: &mut u32,
        total_files: u32,
    ) -> Result<(), AppError> {
        // Handle case where dest is a directory
        let final_dst: PathBuf = if dest.is_dir() {
            if let Some(filename) = source.file_name() {
                let new_dest: PathBuf = dest.join(filename);
                new_dest
            } else {
                let err_kind: ErrorKind = ErrorKind::InvalidInput;
                let err_msg: &'static str = "Cannot determine filename from source.";

                return Err(Self::error(err_kind, err_msg));
            }
        } else {
            let new_dest: PathBuf = dest.to_path_buf();
            new_dest
        };

        // Create parent directory if it doesn't exist
        if let Some(parent) = final_dst.parent()
            && !parent.exists()
        {
            TokioFs::create_dir_all(parent).await?;
        }

        // Get file size for progress tracking
        let metadata: Metadata = TokioFs::metadata(source).await?;
        let file_size: u64 = metadata.len();

        // Report progress before starting file copy
        self.report_progress(
            *current_bytes,
            total_bytes,
            source,
            files_completed,
            total_files,
        )
        .await?;

        let mut src_file: File = TokioFs::File::open(source).await?;
        let mut dst_file: File = TokioFs::File::create(&final_dst).await?;

        // 64KB buffer
        let mut buffer: Vec<u8> = vec![0; BUFFER_SIZE];
        let mut copied: u64 = 0;

        let kib: u64 = 1024 * 1024;
        let size: u64 = std::cmp::min(kib, file_size / 10);
        let optimal_interval: u64 = std::cmp::max(size, 1);

        'copy_file_bytes: loop {
            let bytes_read: usize = src_file.read(&mut buffer).await?;

            // Check for cancellation before starting
            if self.cancel_token.is_cancelled() {
                let err_kind: ErrorKind = ErrorKind::Interrupted;
                let err_msg: &'static str = "Operation was cancelled.";

                return Err(Self::error(err_kind, err_msg));
            }

            if bytes_read == 0 {
                break 'copy_file_bytes;
            }

            dst_file.write_all(&buffer[..bytes_read]).await?;
            copied += bytes_read as u64;
            *current_bytes += bytes_read as u64;

            // Report progress every 1MB or 10% of file
            if copied.is_multiple_of(optimal_interval) {
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

        *files_completed += 1;

        // Final progress report for this file
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

    async fn move_file_with_progress(
        &self,
        source: &PathBuf,
        dest: &Path,
        current_bytes: &mut u64,
        total_bytes: u64,
        files_completed: &mut u32,
        total_files: u32,
    ) -> Result<(), AppError> {
        // Handle case where dest is a directory
        let final_dst: PathBuf = if dest.is_dir() {
            if let Some(filename) = source.file_name() {
                let new_path: PathBuf = dest.join(filename);
                new_path
            } else {
                let ekind: ErrorKind = ErrorKind::InvalidInput;
                let emsg: &'static str =
                    "Cannot determine filename from source for move operation.";
                let err: Error = Error::new(ekind, emsg);
                let app_err: AppError = AppError::Io(err);

                return Err(app_err);
            }
        } else {
            let new_path: PathBuf = dest.to_path_buf();
            new_path
        };

        // Create parent directory if it doesn't exist
        if let Some(parent) = final_dst.parent()
            && !parent.exists()
        {
            TokioFs::create_dir_all(parent).await?;
        }

        // Get file size for progress tracking
        let metadata: Metadata = TokioFs::metadata(source).await?;
        let file_size: u64 = metadata.len();

        *files_completed += 1;

        // Report progress before starting move operation
        self.report_progress(
            *current_bytes,
            total_bytes,
            source,
            files_completed,
            total_files,
        )
        .await?;

        // Check for cancellation before starting
        if self.cancel_token.is_cancelled() {
            let err_kind: ErrorKind = ErrorKind::Interrupted;
            let err_msg: &'static str = "Operation was cancelled.";

            return Err(Self::error(err_kind, err_msg));
        }

        // Try efficient rename first (same filesystem)
        if matches!(TokioFs::rename(source, &final_dst).await, Ok(())) {
            // Rename sucessful - update progress instantly
            *current_bytes += file_size;

            *files_completed += 1;

            // Report completion for this file
            self.report_progress(
                *current_bytes,
                total_bytes,
                source,
                files_completed,
                total_files,
            )
            .await?;
        } else {
            // Rename failed, fall back to copy with progress + delete
            self.copy_file_with_progress(
                source,
                &final_dst,
                current_bytes,
                total_bytes,
                files_completed,
                total_files,
            )
            .await?;

            // Delete source after sucessfuly copy
            if source.is_file() {
                TokioFs::remove_file(source).await?;
            } else if source.is_dir() {
                TokioFs::remove_dir_all(source).await?;
            }
        }

        Ok(())
    }

    /// Rename file or directory with progress reporting
    async fn rename_with_progress(
        &self,
        source: &PathBuf,
        new_name: &str,
        current_bytes: &mut u64,
        total_bytes: u64,
        files_completed: &mut u32,
        total_files: u32,
    ) -> Result<(), AppError> {
        // Validate source exists
        if !source.exists() {
            let ekind: ErrorKind = ErrorKind::NotFound;
            let emsg: String = format!("Source path does not exist: {}", source.display());
            let err: Error = Error::new(ekind, emsg);
            let app_err: AppError = AppError::Io(err);

            return Err(app_err);
        }

        // Get parent directory
        let parent: &Path = source.parent().ok_or_else(|| {
            let ekind: ErrorKind = ErrorKind::InvalidInput;
            let emsg: &'static str = "Cannot rename root directory";
            let err: Error = Error::new(ekind, emsg);
            let app_err: AppError = AppError::Io(err);

            app_err
        })?;

        let new_path: PathBuf = parent.join(new_name);

        // Get file size for progress tracking
        let metadata: Metadata = TokioFs::metadata(source).await?;
        let file_size: u64 = metadata.len();

        // Report progress before starting rename
        self.report_progress(
            *current_bytes,
            total_bytes,
            source,
            files_completed,
            total_files,
        )
        .await?;

        // Check for cancellation before starting
        if self.cancel_token.is_cancelled() {
            let err_kind: ErrorKind = ErrorKind::Interrupted;
            let err_msg: &'static str = "Operation was cancelled.";

            return Err(Self::error(err_kind, err_msg));
        }

        // Perform rename operation
        TokioFs::rename(source, &new_path).await?;

        // Update progress after successful rename
        *current_bytes += file_size;

        *files_completed += 1;

        // Report final progress
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

    #[inline]
    fn error(err_kind: ErrorKind, err_msg: &'static str) -> AppError {
        let err: Error = Error::new(err_kind, err_msg);
        let app_err: AppError = AppError::Io(err);

        app_err
    }
}
