You're absolutely right! Looking at the current file_ops_task.rs, I can
  see that it's missing several critical methods for Phase 2 progress
  tracking. The current implementation is still at the Phase 1 level - it
  has the basic structure but is missing the progress reporting
  functionality.

  Based on the Design.md plan (lines 421-605), here are the missing methods
  that need to be added to FileOperationTask:

  Missing Methods for Phase 2 Progress Tracking:

  1. Add progress reporting method:

  /// Report progress to UI via task channel
  async fn report_progress(
      &self,
      current_bytes: u64,
      total_bytes: u64,
      current_file: &Path,
      files_completed: u32,
      total_files: u32,
  ) -> Result<(), AppError> {
      let progress_result = TaskResult::FileOperationProgress {
          operation_id: self.operation_id.clone(),
          operation_type: match &self.operation {
              FileOperation::Copy { .. } => "Copy".to_string(),
              FileOperation::Move { .. } => "Move".to_string(),
              FileOperation::Rename { .. } => "Rename".to_string(),
          },
          current_bytes,
          total_bytes,
          current_file: current_file.to_path_buf(),
          files_completed,
          total_files,
          start_time: std::time::Instant::now(), // Should track operation
  start time
          throughput_bps: None, // Calculate based on elapsed time
      };

      self.task_tx.send(progress_result).map_err(|_| {
          AppError::Io(std::io::Error::new(
              std::io::ErrorKind::BrokenPipe,
              "Failed to send progress update",
          ))
      })?;

      Ok(())
  }

  2. Add streaming copy with progress updates:

  /// Copy file with progress reporting using streaming
  async fn copy_file_with_progress(
      &self,
      source: &PathBuf,
      dest: &Path,
      current_bytes: &mut u64,
      total_bytes: u64,
      files_completed: u32,
      total_files: u32,
  ) -> Result<(), AppError> {
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
      if let Some(parent) = final_dest.parent() && !parent.exists() {
          tokio::fs::create_dir_all(parent).await.map_err(AppError::Io)?;
      }

      // Get file size for progress tracking
      let metadata =
  tokio::fs::metadata(source).await.map_err(AppError::Io)?;
      let file_size = metadata.len();

      // Report progress before starting file copy
      self.report_progress(*current_bytes, total_bytes, source,
  files_completed, total_files).await?;

      // Use streaming copy with progress updates
      use tokio::io::{AsyncReadExt, AsyncWriteExt};
      let mut src_file =
  tokio::fs::File::open(source).await.map_err(AppError::Io)?;
      let mut dst_file =
  tokio::fs::File::create(&final_dest).await.map_err(AppError::Io)?;

      let mut buffer = vec![0u8; 64 * 1024]; // 64KB buffer
      let mut copied = 0u64;

      loop {
          let bytes_read = src_file.read(&mut
  buffer).await.map_err(AppError::Io)?;
          if bytes_read == 0 {
              break;
          }


  dst_file.write_all(&buffer[..bytes_read]).await.map_err(AppError::Io)?;
          copied += bytes_read as u64;
          *current_bytes += bytes_read as u64;

          // Report progress every 1MB or 10% of file, whichever is smaller
          if copied % (1024 * 1024).min(file_size / 10).max(1) == 0 {
              self.report_progress(*current_bytes, total_bytes, source,
  files_completed, total_files).await?;
          }
      }

      dst_file.flush().await.map_err(AppError::Io)?;

      // Final progress report for this file
      self.report_progress(*current_bytes, total_bytes, source,
  files_completed + 1, total_files).await?;

      Ok(())
  }

  3. Add the main execute method with progress tracking:

  /// Execute file operation with full progress reporting
  pub async fn execute(&self) -> Result<(), AppError> {
      // Calculate total operation size first
      let (total_bytes, total_files) =
  self.calculate_operation_size().await?;
      let mut current_bytes = 0u64;
      let mut files_completed = 0u32;

      // Report initial progress
      match &self.operation {
          FileOperation::Copy { source, .. } | FileOperation::Move { source,
   .. } | FileOperation::Rename { source, .. } => {
              self.report_progress(0, total_bytes, source, 0,
  total_files).await?;
          }
      }

      let result = match &self.operation {
          FileOperation::Copy { source, dest } => {
              self.copy_with_progress(source, dest, &mut current_bytes,
  total_bytes, &mut files_completed, total_files).await
          }
          FileOperation::Move { source, dest } => {
              self.move_with_progress(source, dest, &mut current_bytes,
  total_bytes, &mut files_completed, total_files).await
          }
          FileOperation::Rename { source, new_name } => {
              self.rename_file_or_directory(source, new_name).await?;
              self.report_progress(total_bytes, total_bytes, source, 1,
  1).await?;
              Ok(())
          }
      };

      // Send completion result
      let completion_result = TaskResult::FileOperationComplete {
          operation_id: self.operation_id.clone(),
          result: result.clone(),
      };

      let _ = self.task_tx.send(completion_result);
      result
  }

  4. Add cancellation token support:

  use tokio_util::sync::CancellationToken;

  // Add to FileOperationTask struct:
  pub struct FileOperationTask {
      pub operation_id: String,
      pub operation: FileOperation,
      pub task_tx: mpsc::UnboundedSender<TaskResult>,
      pub cancel_token: CancellationToken, // NEW
  }

  5. Also need to add the dependency to Cargo.toml:

  tokio-util = { version = "0.7", features = ["sync"] }

  These are the core missing pieces for Phase 2 progress tracking. Should I
  provide the complete diffs for all these methods, or would you like to
  implement them step by step?