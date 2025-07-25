use crate::error::{ClipError, ClipResult};
use crate::item::{ClipboardItem, ClipboardOperation};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct PasteOperation {
    pub item_id: String,
    pub source_path: PathBuf,
    pub destination_path: PathBuf,
    pub operation_type: ClipboardOperation,
    pub file_operation: FileOperation,
}

impl PasteOperation {
    pub fn new(item: &ClipboardItem, dest_dir: PathBuf) -> ClipResult<Self> {
        let file_name = item
            .source_path
            .file_name()
            .ok_or_else(|| ClipError::InvalidPath(item.source_path.clone()))?;

        let destination_path = dest_dir.join(file_name);

        let file_operation = match item.operation {
            ClipboardOperation::Copy => FileOperation::Copy {
                source: item.source_path.clone(),
                dest: destination_path.clone(),
            },
            ClipboardOperation::Move => FileOperation::Move {
                source: item.source_path.clone(),
                dest: destination_path.clone(),
            },
        };

        Ok(Self {
            item_id: item.id.clone(),
            source_path: item.source_path.clone(),
            destination_path,
            operation_type: item.operation,
            file_operation,
        })
    }
}

#[derive(Debug, Clone)]
pub enum FileOperation {
    Copy { source: PathBuf, dest: PathBuf },
    Move { source: PathBuf, dest: PathBuf },
}

impl FileOperation {
    pub fn source_path(&self) -> &PathBuf {
        match self {
            FileOperation::Copy { source, .. } => source,
            FileOperation::Move { source, .. } => source,
        }
    }

    pub fn dest_path(&self) -> &PathBuf {
        match self {
            FileOperation::Copy { dest, .. } => dest,
            FileOperation::Move { dest, .. } => dest,
        }
    }

    pub fn operation_name(&self) -> &'static str {
        match self {
            FileOperation::Copy { .. } => "Copy",
            FileOperation::Move { .. } => "Move",
        }
    }
}
