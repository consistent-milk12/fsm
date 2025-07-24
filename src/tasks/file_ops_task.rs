//! src/tasks/file_ops_task.rs
//! ============================================================================
//! # File Operations Task: Background file operations with progress tracking
//!
//! Handles copy, move, and rename operations asynchronously to prevent UI
//! blocking during large file operations.

use crate::controller::event_loop::TaskResult;
use crate::error::AppError;
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;
use uuid::Uuid;

/// File operation task for background processing
#[derive(Debug)]
pub struct FileOperationTask {
    pub operation_id: String,
    pub operation: FileOperation,
    pub task_tx: mpsc::UnboundedSender<TaskResult>,
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

impl FileOperationTask {
    /// Create new file operation task with unique ID
    pub fn new(operation: FileOperation, task_tx: mpsc::UnboundedSender<TaskResult>) -> Self {
        Self {
            operation_id: Uuid::new_v4().to_string(),
            operation,
            task_tx,
        }
    }

    /// Execute the file operation asynchronously
    pub async fn execute(self) -> Result<(), AppError> {
        let result = match &self.operation {
            FileOperation::Copy { source, dest } => self.copy_file_or_directory(source, dest).await,
            FileOperation::Move { source, dest } => self.move_file_or_directory(source, dest).await,
            FileOperation::Rename { source, new_name } => {
                self.rename_file_or_directory(source, new_name).await
            }
        };

        // Send completion notification
        let task_result = TaskResult::FileOperationComplete {
            operation_id: self.operation_id,
            result: result.clone(),
        };

        if let Err(e) = self.task_tx.send(task_result) {
            eprintln!("Failed to send task result: {e}");
        }

        result
    }

    /// Copy file or directory recursively
    async fn copy_file_or_directory(&self, source: &PathBuf, dest: &Path) -> Result<(), AppError> {
        if !source.exists() {
            return Err(AppError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Source path does not exist: {}", source.display()),
            )));
        }

        if source.is_file() {
            self.copy_file(source, dest).await
        } else if source.is_dir() {
            self.copy_directory(source, dest).await
        } else {
            Err(AppError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Unsupported file type: {}", source.display()),
            )))
        }
    }

    /// Copy single file
    async fn copy_file(&self, source: &PathBuf, dest: &Path) -> Result<(), AppError> {
        // Handle case where dest is a directory
        let final_dest = if dest.is_dir() {
            if let Some(filename) = source.file_name() {
                dest.join(filename)
            } else {
                return Err(AppError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Cannot determine filename from source",
                )));
            }
        } else {
            dest.to_path_buf()
        };

        // Create parent directory if it doesn't exist
        if let Some(parent) = final_dest.parent()
            && !parent.exists()
        {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(AppError::Io)?;
        }

        tokio::fs::copy(source, &final_dest)
            .await
            .map_err(AppError::Io)?;

        Ok(())
    }

    /// Copy directory recursively
    fn copy_directory<'a>(
        &'a self,
        source: &'a PathBuf,
        dest: &'a Path,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), AppError>> + Send + 'a>>
    {
        Box::pin(async move {
            // Create destination directory
            let dest_dir = if dest.exists() && dest.is_dir() {
                if let Some(dir_name) = source.file_name() {
                    dest.join(dir_name)
                } else {
                    return Err(AppError::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "Cannot determine directory name from source",
                    )));
                }
            } else {
                dest.to_path_buf()
            };

            tokio::fs::create_dir_all(&dest_dir)
                .await
                .map_err(AppError::Io)?;

            // Copy all entries in the directory
            let mut entries = tokio::fs::read_dir(source).await.map_err(AppError::Io)?;
            while let Some(entry) = entries.next_entry().await.map_err(AppError::Io)? {
                let entry_path = entry.path();
                let dest_path = dest_dir.join(entry.file_name());

                if entry_path.is_file() {
                    self.copy_file(&entry_path, &dest_path).await?;
                } else if entry_path.is_dir() {
                    self.copy_directory(&entry_path, &dest_path).await?;
                }
            }

            Ok(())
        })
    }

    /// Move file or directory
    async fn move_file_or_directory(&self, source: &PathBuf, dest: &Path) -> Result<(), AppError> {
        if !source.exists() {
            return Err(AppError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Source path does not exist: {}", source.display()),
            )));
        }

        // Handle case where dest is a directory
        let final_dest = if dest.is_dir() {
            if let Some(filename) = source.file_name() {
                dest.join(filename)
            } else {
                return Err(AppError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Cannot determine filename from source",
                )));
            }
        } else {
            dest.to_path_buf()
        };

        // Create parent directory if it doesn't exist
        if let Some(parent) = final_dest.parent()
            && !parent.exists()
        {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(AppError::Io)?;
        }

        // Try rename first (efficient for same filesystem)
        match tokio::fs::rename(source, &final_dest).await {
            Ok(()) => Ok(()),
            Err(_) => {
                // If rename fails, fall back to copy + delete
                self.copy_file_or_directory(source, &final_dest).await?;
                if source.is_file() {
                    tokio::fs::remove_file(source).await.map_err(AppError::Io)?;
                } else if source.is_dir() {
                    tokio::fs::remove_dir_all(source)
                        .await
                        .map_err(AppError::Io)?;
                }
                Ok(())
            }
        }
    }

    /// Rename file or directory
    async fn rename_file_or_directory(
        &self,
        source: &PathBuf,
        new_name: &str,
    ) -> Result<(), AppError> {
        if !source.exists() {
            return Err(AppError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Source path does not exist: {}", source.display()),
            )));
        }

        let parent = source.parent().ok_or_else(|| {
            AppError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Cannot rename root directory",
            ))
        })?;

        let new_path = parent.join(new_name);

        tokio::fs::rename(source, &new_path)
            .await
            .map_err(AppError::Io)?;

        Ok(())
    }
}
