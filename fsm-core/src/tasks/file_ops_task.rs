//! High-performance file operations task with proper event loop integration
//!
//! This module provides async file operations that integrate seamlessly with our
//! modular action dispatcher and enhanced event loop. Features:
//! - Progress reporting with cancellation support
//! - Memory-efficient streaming for large files
//! - Proper error handling and resource cleanup
//! - Integration with TaskResult::FileOperation variants

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::time::Instant;
use tokio::fs as TokioFs;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use crate::controller::event_loop::{FileOperationType, TaskResult};
use crate::error::AppError;

const BUFFER_SIZE: usize = 64 * 1024; // 64KB chunks
const PROGRESS_INTERVAL: u64 = 1024 * 1024; // Report every 1MB

/// High-performance file operation task
pub struct FileOperationTask {
    pub operation_id: String,
    pub operation: FileOperation,
    pub task_tx: UnboundedSender<TaskResult>,
    pub cancel_token: CancellationToken,
}

#[derive(Debug, Clone)]
pub enum FileOperation {
    Copy { source: PathBuf, dest: PathBuf },
    Move { source: PathBuf, dest: PathBuf },
    Delete { path: PathBuf },
    CreateFile { path: PathBuf },
    CreateDirectory { path: PathBuf },
    Rename { source: PathBuf, new_name: String },
}

impl FileOperationTask {
    pub fn new(
        operation: FileOperation,
        task_tx: UnboundedSender<TaskResult>,
        cancel_token: CancellationToken,
    ) -> Self {
        Self {
            operation_id: nanoid::nanoid!(),
            operation,
            task_tx,
            cancel_token,
        }
    }

    /// Execute the file operation with progress reporting
    pub async fn execute(&self) -> Result<()> {
        let start_time = Instant::now();

        // Check cancellation before starting
        if self.cancel_token.is_cancelled() {
            return Err(anyhow::anyhow!("Operation cancelled"));
        }

        let result = match &self.operation {
            FileOperation::Copy { source, dest } => self.copy_operation(source, dest).await,
            FileOperation::Move { source, dest } => self.move_operation(source, dest).await,
            FileOperation::Delete { path } => self.delete_operation(path).await,
            FileOperation::CreateFile { path } => self.create_file_operation(path).await,
            FileOperation::CreateDirectory { path } => self.create_dir_operation(path).await,
            FileOperation::Rename { source, new_name } => {
                self.rename_operation(source, new_name).await
            }
        };

        let execution_time = start_time.elapsed();
        let operation_type = self.get_operation_type();

        // Send completion result
        let completion = TaskResult::FileOperation {
            operation_id: self.operation_id.clone(),
            operation_type,
            result: result.map_err(|e| {
                AppError::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                ))
            }),
            execution_time,
        };

        if let Err(e) = self.task_tx.send(completion) {
            warn!("Failed to send task completion: {}", e);
        }

        Ok(())
    }

    async fn copy_operation(&self, source: &Path, dest: &Path) -> Result<()> {
        debug!(
            "Starting copy operation: {} -> {}",
            source.display(),
            dest.display()
        );

        if source.is_file() {
            self.copy_file_with_progress(source, dest).await
        } else if source.is_dir() {
            self.copy_directory_recursive(source, dest).await
        } else {
            Err(anyhow::anyhow!(
                "Source path does not exist: {}",
                source.display()
            ))
        }
    }

    async fn copy_file_with_progress(&self, source: &Path, dest: &Path) -> Result<()> {
        // Determine final destination
        let final_dest = if dest.is_dir() {
            dest.join(source.file_name().context("Invalid source filename")?)
        } else {
            dest.to_path_buf()
        };

        // Create parent directories
        if let Some(parent) = final_dest.parent() {
            TokioFs::create_dir_all(parent).await.with_context(|| {
                format!("Failed to create parent directory: {}", parent.display())
            })?;
        }

        // Get file size for progress tracking
        let metadata = TokioFs::metadata(source)
            .await
            .with_context(|| format!("Failed to read metadata: {}", source.display()))?;
        let file_size = metadata.len();

        // Open files
        let mut src_file = TokioFs::File::open(source)
            .await
            .with_context(|| format!("Failed to open source: {}", source.display()))?;
        let mut dst_file = TokioFs::File::create(&final_dest)
            .await
            .with_context(|| format!("Failed to create destination: {}", final_dest.display()))?;

        // Copy with progress reporting
        let mut buffer = vec![0u8; BUFFER_SIZE];
        let mut copied = 0u64;
        let mut last_progress = 0u64;

        while copied < file_size {
            self.check_cancellation()?;

            let bytes_read = src_file
                .read(&mut buffer)
                .await
                .context("Failed to read from source file")?;

            if bytes_read == 0 {
                break;
            }

            dst_file
                .write_all(&buffer[..bytes_read])
                .await
                .context("Failed to write to destination file")?;

            copied += bytes_read as u64;

            // Report progress at intervals
            if copied - last_progress >= PROGRESS_INTERVAL || copied == file_size {
                self.report_progress(copied, file_size, source).await?;
                last_progress = copied;
            }
        }

        dst_file
            .flush()
            .await
            .context("Failed to flush destination file")?;
        info!("Copy completed: {} bytes", copied);
        Ok(())
    }

    async fn copy_directory_recursive(&self, source: &Path, dest: &Path) -> Result<()> {
        let dest_dir = if dest.exists() && dest.is_dir() {
            dest.join(
                source
                    .file_name()
                    .context("Invalid source directory name")?,
            )
        } else {
            dest.to_path_buf()
        };

        TokioFs::create_dir_all(&dest_dir).await.with_context(|| {
            format!(
                "Failed to create destination directory: {}",
                dest_dir.display()
            )
        })?;

        let mut entries = TokioFs::read_dir(source)
            .await
            .with_context(|| format!("Failed to read source directory: {}", source.display()))?;

        while let Some(entry) = entries.next_entry().await? {
            self.check_cancellation()?;

            let entry_path = entry.path();
            let relative_path = entry_path
                .strip_prefix(source)
                .context("Failed to get relative path")?;
            let dest_path = dest_dir.join(relative_path);

            if entry_path.is_file() {
                self.copy_file_with_progress(&entry_path, &dest_path)
                    .await?;
            } else if entry_path.is_dir() {
                // Use Box::pin to handle async recursion
                Box::pin(self.copy_directory_recursive(&entry_path, &dest_path.parent().unwrap()))
                    .await?;
            }
        }

        Ok(())
    }

    async fn move_operation(&self, source: &Path, dest: &Path) -> Result<()> {
        debug!(
            "Starting move operation: {} -> {}",
            source.display(),
            dest.display()
        );

        let final_dest = if dest.is_dir() {
            dest.join(source.file_name().context("Invalid source filename")?)
        } else {
            dest.to_path_buf()
        };

        // Try rename first (fast path for same filesystem)
        match TokioFs::rename(source, &final_dest).await {
            Ok(()) => {
                info!("Move completed via rename");
                Ok(())
            }
            Err(_) => {
                // Fall back to copy + delete
                debug!("Rename failed, falling back to copy + delete");
                self.copy_operation(source, dest).await?;
                self.delete_operation(source).await?;
                info!("Move completed via copy + delete");
                Ok(())
            }
        }
    }

    async fn delete_operation(&self, path: &Path) -> Result<()> {
        debug!("Starting delete operation: {}", path.display());

        if !path.exists() {
            return Err(anyhow::anyhow!("Path does not exist: {}", path.display()));
        }

        if path.is_file() {
            TokioFs::remove_file(path)
                .await
                .with_context(|| format!("Failed to delete file: {}", path.display()))?;
        } else if path.is_dir() {
            TokioFs::remove_dir_all(path)
                .await
                .with_context(|| format!("Failed to delete directory: {}", path.display()))?;
        }

        info!("Delete completed: {}", path.display());
        Ok(())
    }

    async fn create_file_operation(&self, path: &Path) -> Result<()> {
        debug!("Creating file: {}", path.display());

        if let Some(parent) = path.parent() {
            TokioFs::create_dir_all(parent).await.with_context(|| {
                format!("Failed to create parent directory: {}", parent.display())
            })?;
        }

        TokioFs::File::create(path)
            .await
            .with_context(|| format!("Failed to create file: {}", path.display()))?;

        info!("File created: {}", path.display());
        Ok(())
    }

    async fn create_dir_operation(&self, path: &Path) -> Result<()> {
        debug!("Creating directory: {}", path.display());

        TokioFs::create_dir_all(path)
            .await
            .with_context(|| format!("Failed to create directory: {}", path.display()))?;

        info!("Directory created: {}", path.display());
        Ok(())
    }

    async fn rename_operation(&self, source: &Path, new_name: &str) -> Result<()> {
        debug!("Renaming {} to {}", source.display(), new_name);

        let parent = source.parent().context("Cannot rename root directory")?;
        let new_path = parent.join(new_name);

        TokioFs::rename(source, &new_path)
            .await
            .with_context(|| format!("Failed to rename {} to {}", source.display(), new_name))?;

        info!(
            "Rename completed: {} -> {}",
            source.display(),
            new_path.display()
        );
        Ok(())
    }

    async fn report_progress(&self, current: u64, total: u64, current_file: &Path) -> Result<()> {
        let progress = TaskResult::Progress {
            task_id: 0, // File operations use operation_id instead
            current,
            total,
            message: Some(format!("Processing: {}", current_file.display())),
        };

        self.task_tx
            .send(progress)
            .map_err(|e| anyhow::anyhow!("Failed to send progress: {}", e))?;

        Ok(())
    }

    fn check_cancellation(&self) -> Result<()> {
        if self.cancel_token.is_cancelled() {
            Err(anyhow::anyhow!("Operation cancelled"))
        } else {
            Ok(())
        }
    }

    fn get_operation_type(&self) -> FileOperationType {
        match &self.operation {
            FileOperation::Copy { .. } => FileOperationType::Copy,
            FileOperation::Move { .. } => FileOperationType::Move,
            FileOperation::Delete { .. } => FileOperationType::Delete,
            FileOperation::CreateFile { .. } | FileOperation::CreateDirectory { .. } => {
                FileOperationType::Create
            }
            FileOperation::Rename { .. } => FileOperationType::Rename,
        }
    }
}

/// Spawn a file operation task
pub fn spawn_file_operation(
    operation: FileOperation,
    task_tx: UnboundedSender<TaskResult>,
    cancel_token: CancellationToken,
) -> String {
    let task = FileOperationTask::new(operation, task_tx, cancel_token);
    let operation_id = task.operation_id.clone();

    tokio::spawn(async move {
        if let Err(e) = task.execute().await {
            error!("File operation failed: {}", e);
        }
    });

    operation_id
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn test_file_copy() {
        let temp_dir = TempDir::new().unwrap();
        let source = temp_dir.path().join("source.txt");
        let dest = temp_dir.path().join("dest.txt");

        // Create source file
        TokioFs::write(&source, b"test content").await.unwrap();

        let (task_tx, mut task_rx) = mpsc::unbounded_channel();
        let cancel_token = CancellationToken::new();

        let operation = FileOperation::Copy {
            source: source.clone(),
            dest: dest.clone(),
        };

        let task = FileOperationTask::new(operation, task_tx, cancel_token);
        task.execute().await.unwrap();

        // Verify file was copied
        let content = TokioFs::read_to_string(&dest).await.unwrap();
        assert_eq!(content, "test content");

        // Check that completion message was sent
        if let Some(TaskResult::FileOperation {
            operation_type,
            result,
            ..
        }) = task_rx.recv().await
        {
            assert!(matches!(operation_type, FileOperationType::Copy));
            assert!(result.is_ok());
        } else {
            panic!("Expected FileOperation task result");
        }
    }
}
