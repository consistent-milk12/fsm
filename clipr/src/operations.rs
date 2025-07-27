//! Extreme performance paste operations with zero-allocation hot paths and SIMD acceleration

use crate::error::{ClipError, ClipResult};
use crate::item::{ClipBoardItem, ClipBoardOperation};
use compact_str::CompactString;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

/// Global atomic counter for operation tracking
static OPERATION_COUNTER: AtomicU64 = AtomicU64::new(1);

/// High-performance paste operation with zero-allocation design
#[derive(Debug, Clone)]
pub struct PasteOperation {
    /// Unique operation ID for tracking (8 bytes vs 36 bytes for UUID string)
    pub operation_id: u64,

    /// Clipboard item ID (integer for performance)
    pub item_id: u64,

    /// Memory-optimized source path
    pub source_path: CompactString,

    /// Memory-optimized destination path
    pub destination_path: CompactString,

    /// Operation type with minimal memory footprint
    pub operation_type: ClipBoardOperation,

    /// File operation details
    pub file_operation: FileOperation,

    /// Operation priority for scheduling
    pub priority: OperationPriority,

    /// Estimated operation size for progress tracking
    pub estimated_size: u64,
}

impl PasteOperation {
    /// Create paste operation with path validation and optimization
    pub fn new(item: &ClipBoardItem, dest_dir: PathBuf) -> ClipResult<Self> {
        // Fast path validation with SIMD-optimized filename extraction
        let file_name = Self::extract_filename_simd(&item.source_path)?;

        // Construct destination path efficiently
        let destination_path = Self::build_destination_path(dest_dir, &file_name)?;

        // Create optimized file operation
        let file_operation = match item.operation {
            ClipBoardOperation::Copy => FileOperation::Copy {
                source: CompactString::from(item.source_path.as_str()),
                dest: destination_path.clone(),
                preserve_attrs: true,
                verify_integrity: true,
            },
            ClipBoardOperation::Move => FileOperation::Move {
                source: CompactString::from(item.source_path.as_str()),
                dest: destination_path.clone(),
                atomic_move: true,
                cleanup_source: true,
            },
        };

        Ok(Self {
            operation_id: OPERATION_COUNTER.fetch_add(1, Ordering::Relaxed),
            item_id: item.id,
            source_path: item.source_path.clone(),
            destination_path,
            operation_type: item.operation,
            file_operation,
            priority: Self::calculate_priority(&item.metadata),
            estimated_size: item.metadata.size,
        })
    }

    /// Create batch paste operations with parallel processing optimization
    pub fn create_batch(items: &[ClipBoardItem], dest_dir: PathBuf) -> Vec<ClipResult<Self>> {
        use rayon::prelude::*;

        items
            .par_iter() // Parallel processing for large batches
            .map(|item| Self::new(item, dest_dir.clone()))
            .collect()
    }

    /// Get operation difficulty score for scheduling optimization
    #[inline]
    pub fn difficulty_score(&self) -> u64 {
        let base_score = match self.operation_type {
            ClipBoardOperation::Copy => 100,
            ClipBoardOperation::Move => 150, // Move is more complex
        };

        // Adjust for file size (larger files are more difficult)
        let size_factor = (self.estimated_size / 1024).min(1000); // Max 1000 point bonus

        base_score + size_factor
    }

    /// Check if operation can be executed in parallel with another
    #[inline]
    pub fn can_parallelize_with(&self, other: &Self) -> bool {
        // Don't parallelize operations on the same source or destination
        if self.source_path == other.source_path || self.destination_path == other.destination_path
        {
            return false;
        }

        // Don't parallelize if one is parent directory of another
        if self.is_path_ancestor(&other.source_path)
            || self.is_path_ancestor(&other.destination_path)
            || other.is_path_ancestor(&self.source_path)
            || other.is_path_ancestor(&self.destination_path)
        {
            return false;
        }

        true
    }

    /// Get estimated completion time in milliseconds
    #[inline]
    pub fn estimated_completion_ms(&self) -> u64 {
        // Base time estimates (in milliseconds)
        let base_time = match self.operation_type {
            ClipBoardOperation::Copy => 100, // 100ms base for copy
            ClipBoardOperation::Move => 50,  // 50ms base for move (faster if same filesystem)
        };

        // Estimate based on file size (assume 100MB/s throughput)
        let size_time = self.estimated_size / (100 * 1024 * 1024 / 1000); // Convert to ms

        base_time + size_time
    }

    /// SIMD-optimized filename extraction
    #[inline]
    fn extract_filename_simd(path: &CompactString) -> ClipResult<CompactString> {
        use memchr::memrchr;

        let path_bytes = path.as_bytes();

        // Use SIMD to find last path separator
        let filename_start = if let Some(sep_pos) =
            memrchr(b'/', path_bytes).or_else(|| memrchr(b'\\', path_bytes))
        {
            sep_pos + 1
        } else {
            0
        };

        if filename_start >= path_bytes.len() {
            return Err(ClipError::InvalidPath(CompactString::from(path.as_str())));
        }

        let filename_bytes = &path_bytes[filename_start..];
        let filename = std::str::from_utf8(filename_bytes)
            .map_err(|_| ClipError::InvalidPath(CompactString::from(path.as_str())))?;

        if filename.is_empty() {
            return Err(ClipError::InvalidPath(CompactString::from(path.as_str())));
        }

        Ok(CompactString::from(filename))
    }

    /// Efficient destination path construction
    #[inline]
    fn build_destination_path(
        dest_dir: PathBuf,
        filename: &CompactString,
    ) -> ClipResult<CompactString> {
        let mut dest_path = dest_dir;
        dest_path.push(filename.as_str());

        Ok(CompactString::from(dest_path.to_string_lossy()))
    }

    /// Calculate operation priority based on file metadata
    #[inline]
    fn calculate_priority(metadata: &crate::item::CompactMetadata) -> OperationPriority {
        // Small files get higher priority for better user experience
        if metadata.size < 1024 * 1024 {
            // < 1MB
            OperationPriority::High
        } else if metadata.size < 100 * 1024 * 1024 {
            // < 100MB
            OperationPriority::Medium
        } else {
            OperationPriority::Low
        }
    }

    /// Check if this path is an ancestor of another path
    #[inline]
    fn is_path_ancestor(&self, other_path: &CompactString) -> bool {
        let self_path = self.source_path.as_str();
        let other = other_path.as_str();

        other.starts_with(self_path)
            && other.len() > self_path.len()
            && (other.as_bytes()[self_path.len()] == b'/'
                || other.as_bytes()[self_path.len()] == b'\\')
    }
}

/// Operation priority for scheduling optimization
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)] // Single byte representation
pub enum OperationPriority {
    Low = 0,
    Medium = 1,
    High = 2,
    Critical = 3,
}

/// High-performance file operation with memory-efficient design
#[derive(Debug, Clone)]
pub enum FileOperation {
    Copy {
        source: CompactString,
        dest: CompactString,
        preserve_attrs: bool,
        verify_integrity: bool,
    },
    Move {
        source: CompactString,
        dest: CompactString,
        atomic_move: bool,
        cleanup_source: bool,
    },
}

impl FileOperation {
    /// Get source path with zero allocation
    #[inline(always)]
    pub fn source_path(&self) -> &str {
        match self {
            FileOperation::Copy { source, .. } => source.as_str(),
            FileOperation::Move { source, .. } => source.as_str(),
        }
    }

    /// Get destination path with zero allocation
    #[inline(always)]
    pub fn dest_path(&self) -> &str {
        match self {
            FileOperation::Copy { dest, .. } => dest.as_str(),
            FileOperation::Move { dest, .. } => dest.as_str(),
        }
    }

    /// Get operation name for UI display
    #[inline(always)]
    pub fn operation_name(&self) -> &'static str {
        match self {
            FileOperation::Copy { .. } => "Copy",
            FileOperation::Move { .. } => "Move",
        }
    }

    /// Get single-character operation code for compact display
    #[inline(always)]
    pub fn operation_code(&self) -> char {
        match self {
            FileOperation::Copy { .. } => 'C',
            FileOperation::Move { .. } => 'M',
        }
    }

    /// Check if operation preserves source file
    #[inline(always)]
    pub fn preserves_source(&self) -> bool {
        matches!(self, FileOperation::Copy { .. })
    }

    /// Check if operation requires atomic execution
    #[inline(always)]
    pub fn requires_atomic(&self) -> bool {
        match self {
            FileOperation::Copy { .. } => false,
            FileOperation::Move { atomic_move, .. } => *atomic_move,
        }
    }

    /// Get operation configuration flags
    #[inline]
    pub fn config_flags(&self) -> OperationFlags {
        match self {
            FileOperation::Copy {
                preserve_attrs,
                verify_integrity,
                ..
            } => OperationFlags {
                preserve_attributes: *preserve_attrs,
                verify_integrity: *verify_integrity,
                atomic_operation: false,
                cleanup_source: false,
            },
            FileOperation::Move {
                atomic_move,
                cleanup_source,
                ..
            } => {
                OperationFlags {
                    preserve_attributes: true, // Always preserve for moves
                    verify_integrity: false,   // Not needed for moves
                    atomic_operation: *atomic_move,
                    cleanup_source: *cleanup_source,
                }
            }
        }
    }

    /// Estimate operation complexity score
    #[inline]
    pub fn complexity_score(&self) -> u32 {
        let base = match self {
            FileOperation::Copy { .. } => 100,
            FileOperation::Move { .. } => 80, // Move can be faster on same filesystem
        };

        let flags = self.config_flags();
        let mut score = base;

        if flags.preserve_attributes {
            score += 20;
        }
        if flags.verify_integrity {
            score += 50;
        }
        if flags.atomic_operation {
            score += 30;
        }

        score
    }
}

/// Operation configuration flags with bit-packed representation
#[derive(Debug, Clone, Copy)]
pub struct OperationFlags {
    pub preserve_attributes: bool,
    pub verify_integrity: bool,
    pub atomic_operation: bool,
    pub cleanup_source: bool,
}

impl OperationFlags {
    /// Pack flags into single byte for efficient storage
    #[inline]
    pub fn pack(&self) -> u8 {
        let mut flags = 0u8;
        if self.preserve_attributes {
            flags |= 0b0001;
        }
        if self.verify_integrity {
            flags |= 0b0010;
        }
        if self.atomic_operation {
            flags |= 0b0100;
        }
        if self.cleanup_source {
            flags |= 0b1000;
        }
        flags
    }

    /// Unpack flags from single byte
    #[inline]
    pub fn unpack(flags: u8) -> Self {
        Self {
            preserve_attributes: flags & 0b0001 != 0,
            verify_integrity: flags & 0b0010 != 0,
            atomic_operation: flags & 0b0100 != 0,
            cleanup_source: flags & 0b1000 != 0,
        }
    }
}

/// Batch operation scheduler for optimal parallel execution
#[derive(Debug)]
pub struct BatchScheduler {
    operations: Vec<PasteOperation>,
    max_parallel: usize,
}

impl BatchScheduler {
    /// Create new batch scheduler with optimal parallelism
    pub fn new(operations: Vec<PasteOperation>) -> Self {
        let max_parallel = num_cpus::get().min(operations.len()).max(1);

        Self {
            operations,
            max_parallel,
        }
    }

    /// Schedule operations for optimal parallel execution
    pub fn schedule(&mut self) -> Vec<Vec<PasteOperation>> {
        // Sort by priority and difficulty for optimal scheduling
        self.operations.sort_by(|a, b| {
            a.priority
                .cmp(&b.priority)
                .then_with(|| a.difficulty_score().cmp(&b.difficulty_score()))
        });

        let mut batches = Vec::new();
        let mut remaining = self.operations.clone();

        while !remaining.is_empty() {
            let mut current_batch = Vec::new();
            let mut i = 0;

            while i < remaining.len() && current_batch.len() < self.max_parallel {
                let can_add = current_batch
                    .iter()
                    .all(|op| remaining[i].can_parallelize_with(op));

                if can_add {
                    current_batch.push(remaining.remove(i));
                } else {
                    i += 1;
                }
            }

            // If we couldn't add anything, take the first operation anyway
            if current_batch.is_empty() && !remaining.is_empty() {
                current_batch.push(remaining.remove(0));
            }

            if !current_batch.is_empty() {
                batches.push(current_batch);
            }
        }

        batches
    }

    /// Get total estimated completion time for all batches
    pub fn total_estimated_time_ms(&self) -> u64 {
        // This would be calculated based on scheduled batches
        // For now, return sum of all operations (worst case)
        self.operations
            .iter()
            .map(|op| op.estimated_completion_ms())
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::item::ClipBoardItem;
    use std::path::PathBuf;

    #[test]
    fn test_paste_operation_creation() {
        // Create test item
        let item = ClipBoardItem::new_copy(PathBuf::from("/test/source.txt")).unwrap();
        let dest_dir = PathBuf::from("/dest");

        // Create paste operation
        let paste_op = PasteOperation::new(&item, dest_dir).unwrap();

        // Verify operation properties
        assert_eq!(paste_op.item_id, item.id);
        assert_eq!(paste_op.operation_type, ClipBoardOperation::Copy);
        assert!(paste_op.destination_path.ends_with("source.txt"));
    }

    #[test]
    fn test_simd_filename_extraction() {
        let path = CompactString::from("/long/path/to/filename.txt");
        let filename = PasteOperation::extract_filename_simd(&path).unwrap();
        assert_eq!(filename, "filename.txt");

        // Test Windows path
        let path = CompactString::from("C:\\Windows\\Path\\file.doc");
        let filename = PasteOperation::extract_filename_simd(&path).unwrap();
        assert_eq!(filename, "file.doc");
    }

    #[test]
    fn test_batch_operations() {
        let items: Vec<_> = (0..10)
            .map(|i| ClipBoardItem::new_copy(PathBuf::from(format!("/test/file_{i}.txt"))).unwrap())
            .collect();

        let dest_dir = PathBuf::from("/dest");
        let operations = PasteOperation::create_batch(&items, dest_dir);

        // Verify all operations created successfully
        assert_eq!(operations.len(), 10);
        assert!(operations.iter().all(|op| op.is_ok()));
    }

    #[test]
    fn test_parallelization_detection() {
        let item1 = ClipBoardItem::new_copy(PathBuf::from("/test/file1.txt")).unwrap();
        let item2 = ClipBoardItem::new_copy(PathBuf::from("/test/file2.txt")).unwrap();

        let op1 = PasteOperation::new(&item1, PathBuf::from("/dest1")).unwrap();
        let op2 = PasteOperation::new(&item2, PathBuf::from("/dest2")).unwrap();

        // These should be parallelizable (different sources and destinations)
        assert!(op1.can_parallelize_with(&op2));

        // Test with same destination
        let op3 = PasteOperation::new(&item2, PathBuf::from("/dest1")).unwrap();
        assert!(!op1.can_parallelize_with(&op3));
    }

    #[test]
    fn test_batch_scheduler() {
        let items: Vec<_> = (0..20)
            .map(|i| ClipBoardItem::new_copy(PathBuf::from(format!("/test/file_{i}.txt"))).unwrap())
            .collect();

        let operations: Vec<_> = items
            .iter()
            .map(|item| {
                PasteOperation::new(item, PathBuf::from(format!("/dest/{}", item.id))).unwrap()
            })
            .collect();

        let mut scheduler = BatchScheduler::new(operations);
        let batches = scheduler.schedule();

        // Verify batches were created
        assert!(!batches.is_empty());

        // Verify all operations are scheduled
        let total_ops: usize = batches.iter().map(|batch| batch.len()).sum();
        assert_eq!(total_ops, 20);
    }

    #[test]
    fn test_operation_flags() {
        let flags = OperationFlags {
            preserve_attributes: true,
            verify_integrity: false,
            atomic_operation: true,
            cleanup_source: false,
        };

        let packed = flags.pack();
        let unpacked = OperationFlags::unpack(packed);

        assert_eq!(flags.preserve_attributes, unpacked.preserve_attributes);
        assert_eq!(flags.verify_integrity, unpacked.verify_integrity);
        assert_eq!(flags.atomic_operation, unpacked.atomic_operation);
        assert_eq!(flags.cleanup_source, unpacked.cleanup_source);
    }
}
