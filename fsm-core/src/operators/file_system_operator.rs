use crate::controller::actions::OperationId;
use crate::controller::event_loop::{FileOperationType, TaskResult};
use crate::error::AppError;
use crate::fs::object_info::{LightObjectInfo, ObjectInfo};
use anyhow::{Context, Result};
use std::cmp::Ordering;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::fs as TokioFs;
use tokio::sync::mpsc::{self, UnboundedSender};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, Span, debug, error, info, instrument, warn};

use std::io::Error as StdIoError;

/// Scan update for streaming directory operations
#[derive(Debug, Clone)]
pub enum ScanUpdate {
    /// New entry discovered (immediate display)
    EntryAdded(ObjectInfo),
    /// Batch of entries processed
    BatchComplete {
        processed: usize,
        total: Option<usize>,
    },
    /// Scanning completed
    ScanComplete {
        total_entries: usize,
        exec: Duration,
    },
    /// Error during scanning
    ScanError(String),
}

///
/// Main -> Event Loop -> Event Handler -> Action Dispatcher -> Operator
///
/// FileSystemOperator handles all background file system operations including
/// directory scanning, file operations, and metadata loading.
///
pub struct FileSystemOperator {
    pub operation_id: String,
    pub operation: FileSystemOperation,
    pub task_tx: UnboundedSender<TaskResult>,
    pub cancel_token: CancellationToken,
}

#[derive(Debug, Clone)]
pub enum FileSystemOperation {
    // File/Directory operations
    Delete {
        path: PathBuf,
    },
    CreateFile {
        path: PathBuf,
    },
    CreateDirectory {
        path: PathBuf,
    },
    Rename {
        source: PathBuf,
        new_name: String,
    },

    // Directory scanning operations
    ScanDirectory {
        path: PathBuf,
        show_hidden: bool,
        scan_mode: ScanMode,
    },
}

#[derive(Debug, Clone)]
pub enum ScanMode {
    /// Fast scan with light metadata only
    Fast,
    /// Streaming scan with progress updates
    Streaming { batch_size: usize },
    /// Two-phase: quick display + background metadata
    TwoPhase,
}

impl FileSystemOperation {
    /// Get operation name for logging
    #[instrument(level = "trace")]
    pub fn operation_name(&self) -> &'static str {
        let name = match self {
            FileSystemOperation::CreateFile { .. } => "create_file",
            FileSystemOperation::CreateDirectory { .. } => "create_directory",
            FileSystemOperation::Rename { .. } => "rename",
            FileSystemOperation::Delete { .. } => "delete",
            FileSystemOperation::ScanDirectory { scan_mode, .. } => match scan_mode {
                ScanMode::Fast => "scan_directory_fast",
                ScanMode::Streaming { .. } => "scan_directory_streaming",
                ScanMode::TwoPhase => "scan_directory_two_phase",
            },
        };

        debug!(operation_name = name, "Retrieved operation name");
        name
    }

    /// Get primary path for logging
    #[instrument(level = "trace")]
    pub fn primary_path(&self) -> &Path {
        let path = match self {
            FileSystemOperation::CreateFile { path }
            | FileSystemOperation::CreateDirectory { path }
            | FileSystemOperation::Rename { source: path, .. }
            | FileSystemOperation::Delete { path }
            | FileSystemOperation::ScanDirectory { path, .. } => path,
        };

        debug!(primary_path = %path.display(), "Retrieved primary path");
        path
    }
}

impl std::fmt::Display for FileSystemOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FileSystemOperation::Delete { path } => write!(f, "Delete({})", path.display()),
            FileSystemOperation::CreateFile { path } => write!(f, "CreateFile({})", path.display()),
            FileSystemOperation::CreateDirectory { path } => {
                write!(f, "CreateDirectory({})", path.display())
            }
            FileSystemOperation::Rename { source, new_name } => {
                write!(f, "Rename({} -> {})", source.display(), new_name)
            }
            FileSystemOperation::ScanDirectory {
                path,
                show_hidden,
                scan_mode,
            } => {
                write!(
                    f,
                    "ScanDirectory({}, hidden: {}, mode: {:?})",
                    path.display(),
                    show_hidden,
                    scan_mode
                )
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
            operation_path = %operation.primary_path().display(),
            operation_id = tracing::field::Empty
        )
    )]
    pub fn new(
        operation: FileSystemOperation,
        task_tx: UnboundedSender<TaskResult>,
        cancel_token: CancellationToken,
    ) -> Self {
        let operation_id = nanoid::nanoid!();

        Span::current().record("operation_id", tracing::field::display(&operation_id));
        debug!(operation_id = %operation_id, "Created file system operation task");

        Self {
            operation_id,
            operation,
            task_tx,
            cancel_token,
        }
    }

    #[instrument(level = "trace", skip(self))]
    pub fn get_operation_type(&self) -> FileOperationType {
        let operation_type = match &self.operation {
            FileSystemOperation::CreateFile { .. }
            | FileSystemOperation::CreateDirectory { .. } => FileOperationType::Create,
            FileSystemOperation::Rename { .. } => FileOperationType::Rename,
            FileSystemOperation::Delete { .. } => FileOperationType::Delete,
            FileSystemOperation::ScanDirectory { .. } => FileOperationType::Scan,
        };

        debug!(operation_type = %operation_type, "Determined operation type");
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
            success,
            entries_count
        ),
        skip(self)
    )]
    pub async fn execute(&self) -> Result<()> {
        let start_time = Instant::now();
        let span = Span::current();

        // Check cancellation before starting
        if self.cancel_token.is_cancelled() {
            warn!(operation_id = %self.operation_id, "Operation cancelled before execution");
            return Err(anyhow::anyhow!("Operation Cancelled"));
        }

        let result = match &self.operation {
            FileSystemOperation::CreateFile { path } => self.create_file_operation(path).await,
            FileSystemOperation::CreateDirectory { path } => self.create_dir_operation(path).await,
            FileSystemOperation::Rename { source, new_name } => {
                self.rename_operation(source, new_name).await
            }
            FileSystemOperation::Delete { path } => self.delete_operation(path).await,
            FileSystemOperation::ScanDirectory {
                path,
                show_hidden,
                scan_mode,
            } => {
                self.scan_directory_operation(path, *show_hidden, scan_mode)
                    .await
            }
        };

        let duration: Duration = start_time.elapsed();
        let success: bool = result.is_ok();

        span.record("success", success);

        if let Err(ref e) = result {
            error!(
                operation_id = %self.operation_id,
                error = %e,
                "File system operation failed"
            );
        } else {
            info!(
                operation_id = %self.operation_id,
                duration_ms = duration.as_millis(),
                "File system operation completed successfully"
            );
        }

        // For non‑directory ops, build and send a completion without consuming `result`
        if !matches!(self.operation, FileSystemOperation::ScanDirectory { .. }) {
            // Determine the operation kind and ID
            let operation_type: FileOperationType = self.get_operation_type();
            let op_id: OperationId = OperationId::from_string(self.operation_id.clone());

            // Reconstruct a new Result<(), Arc<AppError>> by borrowing `result`
            let completion_result: Result<(), Arc<AppError>> = match &result {
                // On success, keep Ok(())
                Ok(()) => Ok(()),

                // On error, wrap only the error message in Arc<AppError>
                Err(e) => {
                    let wrapped: AppError = AppError::Io(StdIoError::other(e.to_string()));
                    Err(Arc::new(wrapped))
                }
            };

            // Build the TaskResult using our reconstructed result
            let completion: TaskResult = TaskResult::FileOperation {
                op_id,
                op_kind: operation_type,
                result: completion_result,
                exec: duration,
            };

            // Send it (still owning `result` for the final return)
            if let Err(e) = self.task_tx.send(completion) {
                warn!(
                    operation_id = %self.operation_id,
                    error = %e,
                    "Failed to send task completion"
                );
            }
        }

        // Finally, return the original result
        result
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
        let span = Span::current();

        if let Some(parent) = path.parent() {
            if !parent.exists() {
                TokioFs::create_dir_all(parent).await.with_context(|| {
                    format!("Failed to create parent directory: {}", parent.display())
                })?;

                span.record("parent_created", true);
                debug!(parent = %parent.display(), "Created parent directory");
            }
        }

        TokioFs::File::create(path)
            .await
            .with_context(|| format!("Failed to create file: {}", path.display()))?;

        info!(path = %path.display(), "File created successfully");
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
        let span = Span::current();

        if let Some(parent) = path.parent() {
            if !parent.exists() {
                span.record("recursive", true);
            }
        }

        TokioFs::create_dir_all(path)
            .await
            .with_context(|| format!("Failed to create directory: {}", path.display()))?;

        info!(path = %path.display(), "Directory created successfully");
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
        let span = Span::current();

        let parent = source.parent().context("Cannot rename root directory")?;
        let new_path = parent.join(new_name);

        span.record("destination", tracing::field::display(&new_path.display()));

        TokioFs::rename(source, &new_path)
            .await
            .with_context(|| format!("Failed to rename {} to {}", source.display(), new_name))?;

        info!(
            source = %source.display(),
            destination = %new_path.display(),
            "Rename completed successfully"
        );
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
        let span = Span::current();

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
            TokioFs::remove_file(path)
                .await
                .with_context(|| format!("Failed to delete file: {}", path.display()))?;

            info!(path = %path.display(), "File deleted successfully");
        } else if path.is_dir() {
            TokioFs::remove_dir_all(path)
                .await
                .with_context(|| format!("Failed to delete directory: {}", path.display()))?;

            info!(path = %path.display(), "Directory deleted successfully");
        }

        Ok(())
    }

    #[instrument(
        name = "scan_directory_operation",
        level = "info",
        fields(
            path = %path.display(),
            show_hidden = show_hidden,
            scan_mode = ?scan_mode,
            entries_found
        ),
        skip(self)
    )]
    async fn scan_directory_operation(
        &self,
        path: &Path,
        show_hidden: bool,
        scan_mode: &ScanMode,
    ) -> Result<()> {
        let span = Span::current();

        info!(
            path = %path.display(),
            show_hidden = show_hidden,
            scan_mode = ?scan_mode,
            "Starting directory scan"
        );

        match scan_mode {
            ScanMode::Fast => {
                let entries = self.scan_directory_fast(path, show_hidden).await?;
                span.record("entries_found", entries.len());

                info!(
                    path = %path.display(),
                    entries_count = entries.len(),
                    "Fast directory scan completed"
                );
            }
            ScanMode::Streaming { batch_size } => {
                let entries = self
                    .scan_directory_streaming(path, show_hidden, *batch_size)
                    .await?;
                span.record("entries_found", entries.len());

                info!(
                    path = %path.display(),
                    entries_count = entries.len(),
                    batch_size = batch_size,
                    "Streaming directory scan completed"
                );
            }
            ScanMode::TwoPhase => {
                let entries = self.scan_directory_two_phase(path, show_hidden).await?;
                span.record("entries_found", entries.len());

                info!(
                    path = %path.display(),
                    entries_count = entries.len(),
                    "Two-phase directory scan completed"
                );
            }
        }

        Ok(())
    }

    /// Fast directory scanning with light metadata only
    #[instrument(
        name = "scan_directory_fast",
        level = "debug",
        fields(
            path = %path.display(),
            show_hidden = show_hidden,
            entries_processed = 0,
            entries_filtered = 0
        ),
        skip(self)
    )]
    async fn scan_directory_fast(&self, path: &Path, show_hidden: bool) -> Result<Vec<ObjectInfo>> {
        let span = Span::current();
        let start_time = Instant::now();

        debug!(path = %path.display(), "Starting fast directory scan");

        let mut entries = Vec::new();
        let mut read_dir = TokioFs::read_dir(path)
            .await
            .with_context(|| format!("Failed to read directory: {}", path.display()))?;

        let mut processed = 0;
        let mut filtered = 0;

        while let Some(entry) = read_dir.next_entry().await? {
            processed += 1;

            // Check cancellation periodically
            if processed % 100 == 0 && self.cancel_token.is_cancelled() {
                warn!("Directory scan cancelled");
                return Err(anyhow::anyhow!("Scan cancelled"));
            }

            let entry_path = entry.path();

            // Filter hidden files
            if !show_hidden {
                if let Some(name) = entry_path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with('.') {
                        filtered += 1;
                        continue;
                    }
                }
            }

            match ObjectInfo::from_path_light(&entry_path).await {
                Ok(light_info) => {
                    entries.push(ObjectInfo::with_placeholder_metadata(light_info));
                }
                Err(e) => {
                    debug!(
                        path = %entry_path.display(),
                        error = %e,
                        "Failed to read entry metadata"
                    );
                }
            }
        }

        // Sort: directories first, then alphabetical (consistent with FSState sorting)
        entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            _ => a.name.cmp(&b.name),
        });

        debug!("Directory entries sorted - {} total entries", entries.len());

        let exec = start_time.elapsed();

        span.record("entries_processed", processed);
        span.record("entries_filtered", filtered);

        // Generate task ID for this scan
        let task_id = self.operation_id.parse::<u64>().unwrap_or(0);

        let task_result = TaskResult::DirectoryLoad {
            task_id,
            path: path.to_path_buf(),
            result: Ok(entries.clone()),
            exec,
        };

        if let Err(e) = self.task_tx.send(task_result) {
            warn!(error = %e, "Failed to send directory scan result");
        }

        info!(
            path = %path.display(),
            entries_found = entries.len(),
            duration_ms = exec.as_millis(),
            "Fast directory scan completed"
        );

        Ok(entries)
    }

    /// Streaming directory scanner with progress updates
    #[instrument(
        name = "scan_directory_streaming",
        level = "debug",
        fields(
            path = %path.display(),
            show_hidden = show_hidden,
            batch_size = batch_size,
            entries_processed = 0,
            batches_sent = 0
        ),
        skip(self)
    )]
    async fn scan_directory_streaming(
        &self,
        path: &Path,
        show_hidden: bool,
        batch_size: usize,
    ) -> Result<Vec<ObjectInfo>> {
        let span = Span::current();
        let start_time = Instant::now();
        let mut entries = Vec::new();
        let mut processed = 0;
        let mut batches_sent = 0;

        debug!(
            path = %path.display(),
            batch_size = batch_size,
            "Starting streaming directory scan"
        );

        let mut read_dir = TokioFs::read_dir(path)
            .await
            .with_context(|| format!("Failed to read directory: {}", path.display()))?;

        let task_id = self.operation_id.parse::<u64>().unwrap_or(0);

        while let Some(entry_result) = read_dir.next_entry().await.transpose() {
            // Check cancellation
            if self.cancel_token.is_cancelled() {
                warn!("Streaming directory scan cancelled");
                return Err(anyhow::anyhow!("Scan cancelled"));
            }

            let entry = match entry_result {
                Ok(e) => e,
                Err(e) => {
                    error!(error = %e, "Failed to read directory entry");
                    continue;
                }
            };

            let entry_path = entry.path();

            // Filter hidden files
            if !show_hidden {
                if let Some(name) = entry_path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with('.') {
                        continue;
                    }
                }
            }

            match ObjectInfo::from_path_light(&entry_path).await {
                Ok(light_info) => {
                    let object_info = ObjectInfo::with_placeholder_metadata(light_info);
                    entries.push(object_info);
                    processed += 1;

                    // Send batch progress
                    if processed % batch_size == 0 {
                        batches_sent += 1;

                        let progress_result = TaskResult::Progress {
                            task_id,
                            pct: processed as f32, // TODO: Calculate proper percentage
                            msg: Some(format!("Scanned {processed} entries")),
                        };

                        if let Err(e) = self.task_tx.send(progress_result) {
                            warn!(error = %e, "Failed to send scan progress");
                        }

                        // Yield for responsiveness
                        tokio::task::yield_now().await;
                    }
                }
                Err(e) => {
                    debug!(
                        path = %entry_path.display(),
                        error = %e,
                        "Failed to read entry metadata"
                    );
                }
            }
        }

        // Sort entries (consistent with FSState sorting)
        entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            _ => a.name.cmp(&b.name),
        });

        debug!("Directory entries sorted - {} total entries", entries.len());

        let exec = start_time.elapsed();

        span.record("entries_processed", processed);
        span.record("batches_sent", batches_sent);

        // Send final completion
        let task_result = TaskResult::DirectoryLoad {
            task_id,
            path: path.to_path_buf(),
            result: Ok(entries.clone()),
            exec,
        };

        if let Err(e) = self.task_tx.send(task_result) {
            warn!(error = %e, "Failed to send directory scan completion");
        }

        info!(
            path = %path.display(),
            entries_found = entries.len(),
            batches_sent = batches_sent,
            duration_ms = exec.as_millis(),
            "Streaming directory scan completed"
        );

        Ok(entries)
    }

    /// Two-phase scanner: immediate display + background metadata loading
    #[instrument(
        name = "scan_directory_two_phase",
        level = "debug",
        fields(
            path = %path.display(),
            show_hidden = show_hidden,
            quick_entries = 0,
            metadata_entries = 0
        ),
        skip(self)
    )]
    async fn scan_directory_two_phase(
        &self,
        path: &Path,
        show_hidden: bool,
    ) -> Result<Vec<ObjectInfo>> {
        let span = Span::current();
        let start_time = Instant::now();

        debug!(path = %path.display(), "Starting two-phase directory scan");

        // Phase 1: Quick scan for immediate display
        let (entries, light_entries) = self.scan_with_light_metadata(path, show_hidden).await?;

        span.record("quick_entries", entries.len());
        span.record("metadata_entries", light_entries.len());

        let task_id = self.operation_id.parse::<u64>().unwrap_or(0);

        // Send quick results for immediate display
        let quick_result = TaskResult::DirectoryLoad {
            task_id,
            path: path.to_path_buf(),
            result: Ok(entries.clone()),
            exec: start_time.elapsed(),
        };

        if let Err(e) = self.task_tx.send(quick_result) {
            warn!(error = %e, "Failed to send quick scan results");
        }

        // Phase 2: Background metadata loading would be spawned here
        // Note: This would typically spawn a separate task for metadata loading
        if !light_entries.is_empty() {
            debug!(
                metadata_entries = light_entries.len(),
                "Would spawn background metadata loading task"
            );
            // TODO: Implement background metadata loading
        }

        info!(
            path = %path.display(),
            quick_entries = entries.len(),
            metadata_entries = light_entries.len(),
            duration_ms = start_time.elapsed().as_millis(),
            "Two-phase directory scan completed"
        );

        Ok(entries)
    }

    /// Scan directory and collect both full entries and light metadata
    #[instrument(
        name = "scan_with_light_metadata",
        level = "debug",
        fields(
            path = %path.display(),
            show_hidden = show_hidden,
            entries_processed = 0,
            entries_filtered = 0
        ),
        skip(self)
    )]
    async fn scan_with_light_metadata(
        &self,
        path: &Path,
        show_hidden: bool,
    ) -> Result<(Vec<ObjectInfo>, Vec<LightObjectInfo>)> {
        let span = Span::current();
        let mut entries = Vec::new();
        let mut light_entries = Vec::new();
        let mut processed = 0;
        let mut filtered = 0;

        let mut read_dir = TokioFs::read_dir(path)
            .await
            .with_context(|| format!("Failed to read directory: {}", path.display()))?;

        while let Some(entry) = read_dir.next_entry().await? {
            processed += 1;

            // Check cancellation periodically
            if processed % 50 == 0 && self.cancel_token.is_cancelled() {
                warn!("Light metadata scan cancelled");
                return Err(anyhow::anyhow!("Scan cancelled"));
            }

            let entry_path = entry.path();

            // Filter hidden files
            if !show_hidden {
                if let Some(name) = entry_path.file_name().and_then(|n| n.to_str()) {
                    if name.starts_with('.') {
                        filtered += 1;
                        continue;
                    }
                }
            }

            match ObjectInfo::from_path_light(&entry_path).await {
                Ok(light_info) => {
                    let object_info = ObjectInfo::with_placeholder_metadata(light_info.clone());
                    entries.push(object_info);
                    light_entries.push(light_info);
                }
                Err(e) => {
                    debug!(
                        path = %entry_path.display(),
                        error = %e,
                        "Failed to read entry light metadata"
                    );
                }
            }
        }

        // Sort entries (consistent with FSState sorting)
        entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            _ => a.name.cmp(&b.name),
        });

        debug!("Directory entries sorted - {} total entries", entries.len());

        span.record("entries_processed", processed);
        span.record("entries_filtered", filtered);

        debug!(
            entries_found = entries.len(),
            light_entries = light_entries.len(),
            "Light metadata scan completed"
        );

        Ok((entries, light_entries))
    }
}

// Convenience functions for spawning operations

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
    operation: FileSystemOperation,
    task_tx: UnboundedSender<TaskResult>,
    cancel_token: CancellationToken,
) -> String {
    let task = FileSystemOperator::new(operation, task_tx, cancel_token);
    let operation_id = task.operation_id.clone();

    // Record the generated operation ID in the span
    Span::current().record("operation_id", tracing::field::display(&operation_id));

    info!(operation_id = %operation_id, "Spawning file system operation task");

    tokio::spawn(
        async move {
            if let Err(e) = task.execute().await {
                error!(
                    operation_id = %task.operation_id,
                    error = %e,
                    "File system operation task failed"
                );
            }
        }
        .instrument(tracing::info_span!(
            "file_system_operation_task",
            operation_id = %operation_id
        )),
    );

    operation_id
}

/// Spawn a directory scan operation
#[instrument(
    name = "spawn_directory_scan",
    level = "info",
    fields(
        path = %path.display(),
        show_hidden = show_hidden,
        scan_mode = ?scan_mode,
        operation_id = tracing::field::Empty
    )
)]
pub fn spawn_directory_scan(
    path: PathBuf,
    show_hidden: bool,
    scan_mode: ScanMode,
    task_tx: UnboundedSender<TaskResult>,
    cancel_token: CancellationToken,
) -> String {
    let operation = FileSystemOperation::ScanDirectory {
        path: path.clone(),
        show_hidden,
        scan_mode: scan_mode.clone(),
    };

    spawn_file_operation(operation, task_tx, cancel_token)
}

/// Spawn a fast directory scan
#[instrument(
    name = "spawn_directory_scan_fast",
    level = "info",
    fields(
        path = %path.display(),
        show_hidden = show_hidden,
        task_id = task_id
    )
)]
pub fn spawn_directory_scan_fast(
    task_id: u64,
    path: PathBuf,
    show_hidden: bool,
    task_tx: UnboundedSender<TaskResult>,
    cancel_token: CancellationToken,
) -> JoinHandle<Result<Vec<ObjectInfo>>> {
    let path_str: PathBuf = path.clone();

    tokio::spawn(
        async move {
            let operation: FileSystemOperation = FileSystemOperation::ScanDirectory {
                path: path.clone(),
                show_hidden,
                scan_mode: ScanMode::Fast,
            };

            let operator: FileSystemOperator =
                FileSystemOperator::new(operation, task_tx, cancel_token);

            match operator.execute().await {
                Ok(_) => {
                    // The execute method already sends the task result
                    // For compatibility, we need to return the entries
                    // This is a bit awkward but maintains the original API
                    Ok(Vec::new()) // TODO: Return actual entries
                }
                Err(e) => Err(e),
            }
        }
        .instrument(tracing::info_span!(
            "directory_scan_fast_task",
            task_id = task_id,
            path = %path_str.display()
        )),
    )
}

/// Spawn a streaming directory scan with progress updates
#[instrument(
    name = "spawn_streaming_directory_scan",
    level = "info",
    fields(
        path = %path.display(),
        show_hidden = show_hidden,
        batch_size = batch_size,
        task_id = task_id
    )
)]
pub fn spawn_streaming_directory_scan(
    task_id: u64,
    path: PathBuf,
    show_hidden: bool,
    batch_size: usize,
    task_tx: UnboundedSender<TaskResult>,
    cancel_token: CancellationToken,
) -> (
    mpsc::UnboundedReceiver<ScanUpdate>,
    JoinHandle<Result<Vec<ObjectInfo>>>,
) {
    let path_str: PathBuf = path.clone();

    let (update_tx, update_rx) = tokio::sync::mpsc::unbounded_channel();

    let handle = tokio::spawn(
        async move {
            let operation: FileSystemOperation = FileSystemOperation::ScanDirectory {
                path: path.clone(),
                show_hidden,
                scan_mode: ScanMode::Streaming { batch_size },
            };

            let operator: FileSystemOperator =
                FileSystemOperator::new(operation, task_tx, cancel_token);

            // Custom streaming implementation with update channel
            let start_time = Instant::now();
            let mut entries = Vec::new();
            let mut processed = 0;

            info!(
                task_id = task_id,
                path = %path.display(),
                batch_size = batch_size,
                "Starting streaming directory scan with updates"
            );

            let mut read_dir: TokioFs::ReadDir = match TokioFs::read_dir(&path).await {
                Ok(rd) => rd,
                Err(e) => {
                    let error_msg = format!("Failed to read directory: {e}");
                    let _ = update_tx.send(ScanUpdate::ScanError(error_msg.clone()));

                    error!(
                        task_id = task_id,
                        path = %path.display(),
                        error = %e,
                        "Failed to open directory for streaming scan"
                    );

                    let app_error: AppError = AppError::Io(e);
                    let task_result: TaskResult = TaskResult::DirectoryLoad {
                        task_id,
                        path: path.clone(),
                        result: Err(Arc::new(app_error)),
                        exec: start_time.elapsed(),
                    };

                    let _ = operator.task_tx.send(task_result);
                    return Err(anyhow::anyhow!("Directory read failed"));
                }
            };

            while let Some(entry_result) = read_dir.next_entry().await.transpose() {
                // Check cancellation
                if operator.cancel_token.is_cancelled() {
                    warn!(task_id = task_id, "Streaming directory scan cancelled");
                    let _ = update_tx.send(ScanUpdate::ScanError("Scan cancelled".to_string()));
                    return Err(anyhow::anyhow!("Scan cancelled"));
                }

                let entry: TokioFs::DirEntry = match entry_result {
                    Ok(e) => e,
                    Err(e) => {
                        let error_msg = format!("Failed to read entry: {e}");
                        let _ = update_tx.send(ScanUpdate::ScanError(error_msg));

                        debug!(
                            task_id = task_id,
                            error = %e,
                            "Failed to read directory entry during streaming scan"
                        );
                        continue;
                    }
                };

                let entry_path: PathBuf = entry.path();

                // Filter hidden files
                if !show_hidden {
                    if let Some(name) = entry_path.file_name().and_then(|n: &OsStr| n.to_str()) {
                        if name.starts_with('.') {
                            continue;
                        }
                    }
                }

                match ObjectInfo::from_path_light(&entry_path).await {
                    Ok(light_info) => {
                        let object_info = ObjectInfo::with_placeholder_metadata(light_info);

                        // Send immediate update for UI
                        let _ = update_tx.send(ScanUpdate::EntryAdded(object_info.clone()));
                        entries.push(object_info);
                        processed += 1;

                        // Send batch progress
                        if processed % batch_size == 0 {
                            let _ = update_tx.send(ScanUpdate::BatchComplete {
                                processed,
                                total: None,
                            });

                            // Report progress to task system
                            let progress_result = TaskResult::Progress {
                                task_id,
                                pct: processed as f32, // TODO: Calculate proper percentage
                                msg: Some(format!("Scanned {processed} entries")),
                            };

                            let _ = operator.task_tx.send(progress_result);

                            debug!(
                                task_id = task_id,
                                processed = processed,
                                "Sent batch progress update"
                            );

                            // Yield for responsiveness
                            tokio::task::yield_now().await;
                        }
                    }
                    Err(e) => {
                        let error_msg = format!("Failed to read {}: {}", entry_path.display(), e);
                        let _ = update_tx.send(ScanUpdate::ScanError(error_msg));

                        debug!(
                            task_id = task_id,
                            path = %entry_path.display(),
                            error = %e,
                            "Failed to read entry metadata during streaming scan"
                        );
                    }
                }
            }

            // Sort entries
            entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
                (true, false) => Ordering::Less,
                (false, true) => Ordering::Greater,
                _ => a.name.cmp(&b.name),
            });

            let exec = start_time.elapsed();

            // Send completion update
            let _ = update_tx.send(ScanUpdate::ScanComplete {
                total_entries: entries.len(),
                exec,
            });

            // Send task completion
            let task_result = TaskResult::DirectoryLoad {
                task_id,
                path: path.clone(),
                result: Ok(entries.clone()),
                exec,
            };
            let _ = operator.task_tx.send(task_result);

            info!(
                task_id = task_id,
                path = %path.display(),
                entries_found = entries.len(),
                duration_ms = exec.as_millis(),
                "Streaming directory scan with updates completed"
            );

            Ok(entries)
        }
        .instrument(tracing::info_span!(
            "streaming_directory_scan_task",
            task_id = task_id,
            path = %path_str.display()
        )),
    );

    (update_rx, handle)
}

/// Spawn a two-phase directory scan
#[instrument(
    name = "spawn_two_phase_directory_scan",
    level = "info",
    fields(
        path = %path.display(),
        show_hidden = show_hidden,
        task_id = task_id
    )
)]
pub fn spawn_two_phase_directory_scan(
    task_id: u64,
    path: PathBuf,
    show_hidden: bool,
    task_tx: UnboundedSender<TaskResult>,
    cancel_token: CancellationToken,
) -> JoinHandle<Result<Vec<ObjectInfo>>> {
    let path_str: PathBuf = path.clone();

    tokio::spawn(
        async move {
            let operation = FileSystemOperation::ScanDirectory {
                path: path.clone(),
                show_hidden,
                scan_mode: ScanMode::TwoPhase,
            };

            let operator = FileSystemOperator::new(operation, task_tx, cancel_token);

            info!(
                task_id = task_id,
                path = %path.display(),
                "Starting two-phase directory scan"
            );

            match operator.execute().await {
                Ok(_) => {
                    info!(
                        task_id = task_id,
                        path = %path.display(),
                        "Two-phase directory scan completed successfully"
                    );
                    // TODO: Return actual entries instead of empty vec
                    Ok(Vec::new())
                }
                Err(e) => {
                    error!(marker = "TWO_PHASE_SCAN_WITH_UPDATES_FAILED", operation_type = "file_system",
                    task_id = task_id,
                    path = %path.display(),
                    error = %e,
                    "Two-phase directory scan failed"
                );
                    Err(e)
                }
            }
        }
        .instrument(tracing::info_span!(
            "two_phase_directory_scan_task",
            task_id = task_id,
            path = %path_str.display()
        )),
    )
}

// Legacy compatibility functions to maintain the original API
// These provide backward compatibility with existing code

/// Legacy function: spawn directory scan (maintains original API)
#[instrument(
    name = "spawn_directory_scan_legacy",
    level = "info",
    fields(
        task_id = task_id,
        path = %path.display(),
        show_hidden = show_hidden
    )
)]
pub fn spawn_directory_scan_legacy(
    task_id: u64,
    path: PathBuf,
    show_hidden: bool,
    task_tx: UnboundedSender<TaskResult>,
) -> JoinHandle<Result<Vec<ObjectInfo>>> {
    let cancel_token = CancellationToken::new();
    spawn_directory_scan_fast(task_id, path, show_hidden, task_tx, cancel_token)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::fs::File;

    async fn create_test_directory() -> Result<TempDir> {
        let temp_dir = TempDir::new()?;
        let path = temp_dir.path();

        // Create test files and directories
        File::create(path.join("file1.txt")).await?;
        File::create(path.join("file2.txt")).await?;
        File::create(path.join(".hidden")).await?;
        TokioFs::create_dir(path.join("subdir")).await?;

        Ok(temp_dir)
    }

    #[tokio::test]
    async fn test_fast_directory_scan() {
        let temp_dir = create_test_directory().await.unwrap();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let cancel_token = CancellationToken::new();

        let operation = FileSystemOperation::ScanDirectory {
            path: temp_dir.path().to_path_buf(),
            show_hidden: false,
            scan_mode: ScanMode::Fast,
        };

        let operator = FileSystemOperator::new(operation, tx, cancel_token);
        let result = operator.execute().await;

        assert!(result.is_ok());

        // Should receive a DirectoryLoad task result
        let task_result = rx.recv().await.unwrap();
        match task_result {
            TaskResult::DirectoryLoad { result, .. } => {
                let entries = result.unwrap();
                // Should have 3 entries (2 files + 1 dir, no hidden file)
                assert_eq!(entries.len(), 3);
            }
            _ => panic!("Expected DirectoryLoad result"),
        }
    }

    #[tokio::test]
    async fn test_scan_with_hidden_files() {
        let temp_dir = create_test_directory().await.unwrap();
        let (tx, mut rx) = mpsc::unbounded_channel();
        let cancel_token = CancellationToken::new();

        let operation = FileSystemOperation::ScanDirectory {
            path: temp_dir.path().to_path_buf(),
            show_hidden: true,
            scan_mode: ScanMode::Fast,
        };

        let operator = FileSystemOperator::new(operation, tx, cancel_token);
        let result = operator.execute().await;

        assert!(result.is_ok());

        let task_result = rx.recv().await.unwrap();
        match task_result {
            TaskResult::DirectoryLoad { result, .. } => {
                let entries = result.unwrap();
                // Should have 4 entries (2 files + 1 dir + 1 hidden file)
                assert_eq!(entries.len(), 4);
            }
            _ => panic!("Expected DirectoryLoad result"),
        }
    }

    #[tokio::test]
    async fn test_create_file_operation() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("new_file.txt");
        let (tx, mut rx) = mpsc::unbounded_channel();
        let cancel_token = CancellationToken::new();

        let operation = FileSystemOperation::CreateFile {
            path: file_path.clone(),
        };

        let operator = FileSystemOperator::new(operation, tx, cancel_token);
        let result = operator.execute().await;

        assert!(result.is_ok());
        assert!(file_path.exists());

        // Should receive a FileOperation task result
        let task_result = rx.recv().await.unwrap();
        match task_result {
            TaskResult::FileOperation { result, .. } => {
                assert!(result.is_ok());
            }
            _ => panic!("Expected FileOperation result"),
        }
    }

    #[tokio::test]
    async fn test_create_directory_operation() {
        let temp_dir = TempDir::new().unwrap();
        let dir_path = temp_dir.path().join("new_directory");
        let (tx, mut rx) = mpsc::unbounded_channel();
        let cancel_token = CancellationToken::new();

        let operation = FileSystemOperation::CreateDirectory {
            path: dir_path.clone(),
        };

        let operator = FileSystemOperator::new(operation, tx, cancel_token);
        let result = operator.execute().await;

        assert!(result.is_ok());
        assert!(dir_path.exists() && dir_path.is_dir());

        let task_result = rx.recv().await.unwrap();
        match task_result {
            TaskResult::FileOperation { result, .. } => {
                assert!(result.is_ok());
            }
            _ => panic!("Expected FileOperation result"),
        }
    }

    #[tokio::test]
    async fn test_cancellation() {
        let temp_dir = create_test_directory().await.unwrap();
        let (tx, _rx) = mpsc::unbounded_channel();
        let cancel_token = CancellationToken::new();

        // Cancel immediately
        cancel_token.cancel();

        let operation = FileSystemOperation::ScanDirectory {
            path: temp_dir.path().to_path_buf(),
            show_hidden: false,
            scan_mode: ScanMode::Fast,
        };

        let operator = FileSystemOperator::new(operation, tx, cancel_token);
        let result = operator.execute().await;

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Operation Cancelled")
        );
    }
}
