use crate::controller::actions::OperationId;
use crate::controller::event_loop::{FileOperationType, TaskResult};
use crate::error::AppError;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::fs as TokioFs;
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, Span, instrument};

use std::io::Error as StdIoError;

///
/// Main -> Event Loop -> Event Handler -> Action Dispatcher -> Operator
///
/// FileSystemOperator should be responsible for all background work this application
/// will perform on the file system.
///
pub struct FileSystemOperator {
    pub operation_id: String,
    pub operation: FileOperation,
    pub task_tx: UnboundedSender<TaskResult>,
    pub cancel_token: CancellationToken,
}

#[derive(Debug, Clone)]
pub enum FileOperation {
    Delete { path: PathBuf },

    CreateFile { path: PathBuf },

    CreateDirectory { path: PathBuf },

    Rename { source: PathBuf, new_name: String },
}

impl FileOperation {
    /// Get opeartion name for logging
    pub fn operation_name(&self) -> &'static str {
        match self {
            FileOperation::CreateFile { .. } => "create_file",

            FileOperation::CreateDirectory { .. } => "create_directory",

            FileOperation::Rename { .. } => "rename",

            FileOperation::Delete { .. } => "delete",
        }
    }

    /// Get primary path for logging
    pub fn primary_path(&self) -> &Path {
        match self {
            FileOperation::CreateFile { path }
            | FileOperation::CreateDirectory { path }
            | FileOperation::Rename { source: path, .. }
            | FileOperation::Delete { path } => path,
        }
    }
}

impl std::fmt::Display for FileOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FileOperation::Delete { path } => write!(f, "Delete({})", path.display()),

            FileOperation::CreateFile { path } => write!(f, "CreateFile({})", path.display()),

            FileOperation::CreateDirectory { path } => {
                write!(f, "CreateDirectory({})", path.display())
            }

            FileOperation::Rename { source, new_name } => {
                write!(f, "Rename({} -> {})", source.display(), new_name)
            }
        }
    }
}

impl FileSystemOperator {
    #[instrument(
        name = "file_operator_new",
        level = "debug",
        fields(
            operation_type = operation.operation_name(),
            operation_path = %operation.primary_path().display()
        )
    )]
    pub fn new(
        operation: FileOperation,
        task_tx: UnboundedSender<TaskResult>,
        cancel_token: CancellationToken,
    ) -> Self {
        let operation_id: String = nanoid::nanoid!();

        tracing::debug!(operation_id = %operation_id, "Created file operation task");

        Self {
            operation_id: nanoid::nanoid!(),
            operation,
            task_tx,
            cancel_token,
        }
    }

    #[instrument(level = "trace", skip(self))]
    pub fn get_operation_type(&self) -> FileOperationType {
        let operation_type = match &self.operation {
            FileOperation::CreateFile { .. } | FileOperation::CreateDirectory { .. } => {
                FileOperationType::Create
            }

            FileOperation::Rename { .. } => FileOperationType::Rename,

            FileOperation::Delete { .. } => FileOperationType::Delete,
        };

        tracing::debug!(operation_type = %operation_type, "Determined operation type");

        operation_type
    }

    #[instrument(
        name = "file_operation_execute",
        level = "info",
        fields(
            operation_id = %self.operation_id,
            operation_type = self.operation.operation_name(),
            operation_path = %self.operation.primary_path().display(),
            duration_ms,
            success
        ),
        skip(self)
    )]
    pub async fn execute(&self) -> Result<()> {
        let start_time: Instant = Instant::now();
        let span: Span = Span::current();

        // Check cancellation before starting
        if self.cancel_token.is_cancelled() {
            tracing::warn!("Operation cancelled before execution");

            return Err(anyhow::anyhow!("Operation Cancelled"));
        }

        let result: Result<()> = match &self.operation {
            FileOperation::CreateFile { path } => self.create_file_operation(path).await,

            FileOperation::CreateDirectory { path } => self.create_dir_operation(path).await,

            FileOperation::Rename { source, new_name } => {
                self.rename_operation(source, new_name).await
            }

            FileOperation::Delete { path } => self.delete_operation(path).await,
        };

        let duration: Duration = start_time.elapsed();
        let success: bool = result.is_ok();

        if let Err(ref e) = result {
            tracing::error!(error = %e, "File operation failed")
        } else {
            tracing::info!(
                duration_ms = duration.as_millis(),
                "File operation completed successfully"
            );
        }

        // Record metrics in span
        span.record("duration_ms", duration.as_millis());
        span.record("success", success);

        let exec: Duration = start_time.elapsed();
        let operation_type: FileOperationType = self.get_operation_type();

        let completion: TaskResult = TaskResult::FileOperation {
            op_id: OperationId::from_string(self.operation_id.clone()),
            op_kind: operation_type,
            result: result
                .map_err(|e: anyhow::Error| AppError::Io(StdIoError::other(e.to_string()))),
            exec,
        };

        if let Err(e) = self.task_tx.send(completion) {
            tracing::warn!(error = %e, "Failed to send task completion");
        }

        Ok(())
    }

    #[instrument(
        name = "create_file_operation",
        level = "info",
        fields(
            path = %path.display(),
            parent_created = false,
        ),
        skip(self)
    )]
    async fn create_file_operation(&self, path: &Path) -> Result<()> {
        let span: Span = Span::current();

        if let Some(parent) = path.parent()
            && !parent.exists()
        {
            TokioFs::create_dir_all(parent).await?;

            span.record("parent_created", true);

            tracing::debug!(parent = %parent.display(), "Created parent directory");
        }

        TokioFs::File::create(path)
            .await
            .with_context(|| format!("Failed to create file: {}", path.display()))?;

        tracing::debug!("File Created: {}", path.display());

        Ok(())
    }

    #[instrument(
        name = "create_directory_operation",
        level = "info",
        fields(
            path = %path.display(),
            recursive = false,
        ),
        skip(self)
    )]
    async fn create_dir_operation(&self, path: &Path) -> Result<()> {
        let span: Span = Span::current();

        if let Some(parent) = path.parent()
            && !parent.exists()
        {
            span.record("recursive", true);
        }

        TokioFs::create_dir_all(path)
            .await
            .with_context(|| format!("Failed to create directory: {}", path.display()))?;

        tracing::debug!("Creating Directory: {}", path.display());

        Ok(())
    }

    #[instrument(
        name = "rename_operation",
        level = "info",
        fields(
            source = %source.display(),
            new_name = %new_name,
            destination = tracing::field::Empty
        ),
        skip(self)
    )]
    async fn rename_operation(&self, source: &Path, new_name: &str) -> Result<()> {
        let span: Span = Span::current();

        let parent: &Path = source.parent().context("Cannot rename root directory")?;
        let new_path: PathBuf = parent.join(new_name);

        span.record("destination", tracing::field::display(&new_path.display()));

        TokioFs::rename(source, &new_path)
            .await
            .with_context(|| format!("Failed to rename {} to {}", source.display(), new_name))?;

        tracing::debug!("Rename completed successfully");

        Ok(())
    }

    #[instrument(
        name = "delete_operation",
        level = "info",
        fields(
            path = %path.display(),
            file_type,
            file_size_bytes
        ),
        skip(self)
    )]
    async fn delete_operation(&self, path: &Path) -> Result<()> {
        let span: Span = Span::current();

        if !path.exists() {
            return Err(anyhow::anyhow!("Path does not exist: {}", path.display()));
        }

        // Record file metadata for better observability
        if let Ok(metadata) = TokioFs::metadata(path).await {
            span.record("file_size_bytes", metadata.len());

            span.record(
                "file_type",
                if metadata.is_dir() {
                    "directory"
                } else {
                    "file"
                },
            );
        }

        if path.is_file() {
            TokioFs::remove_file(path).await?;

            tracing::debug!("File deleted successfully");
        } else if path.is_dir() {
            TokioFs::remove_dir_all(path)
                .await
                .with_context(|| format!("Failed to delete directory: {}", path.display()))?;

            tracing::debug!("Directory deleted successfully");
        }

        Ok(())
    }
}

#[instrument(
    name = "spawn_file_operation",
    level = "info",
    fields(
        operation_type = operation.operation_name(),
        operation_path = %operation.primary_path().display(),
        operation_id = tracing::field::Empty
    )
)]
pub fn spawn_file_operation(
    operation: FileOperation,
    task_tx: UnboundedSender<TaskResult>,
    cancel_token: CancellationToken,
) -> String {
    let task: FileSystemOperator = FileSystemOperator::new(operation, task_tx, cancel_token);
    let operation_id: String = task.operation_id.clone();

    // Record the generated operation ID in the span
    Span::current().record("operation_id", &tracing::field::display(&operation_id));

    tracing::info!(operation_id = %operation_id, "Spawning file operation task");

    tokio::spawn(
        async move {
            if let Err(e) = task.execute().await {
                tracing::error!(
                    operation_id = %task.operation_id,
                    error = %e,
                    "File operation task failed"
                );
            }
        }
        .instrument(tracing::info_span!(
            "file_operation_task",
            operation_id = %operation_id
        )),
    );

    operation_id
}
