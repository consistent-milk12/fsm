use crate::config::ClipboardConfig;
use crate::error::{ClipError, ClipResult};
use crate::item::{ClipboardItem, ClipboardOperation};
use crate::operations::PasteOperation;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct Clipboard {
    items: HashMap<String, ClipboardItem>,
    item_order: Vec<String>,
    config: ClipboardConfig,
    created_at: Instant,
}

impl Clipboard {
    pub fn new(config: ClipboardConfig) -> Self {
        Self {
            items: HashMap::new(),
            item_order: Vec::new(),
            config,
            created_at: Instant::now(),
        }
    }

    /// Add file to clipboard for copy operation
    pub fn add_copy(&mut self, path: PathBuf) -> ClipResult<String> {
        let item: ClipboardItem = ClipboardItem::new_copy(path)?;
        let id: String = item.id.clone();

        self.insert_item(item)?;
        Ok(id)
    }

    /// Add file to clipboard for move operation
    pub fn add_move(&mut self, path: PathBuf) -> ClipResult<String> {
        let item: ClipboardItem = ClipboardItem::new_move(path)?;
        let id: String = item.id.clone();

        self.insert_item(item)?;
        Ok(id)
    }

    /// Remove item from clipboard
    pub fn remove_item(&mut self, id: &str) -> ClipResult<ClipboardItem> {
        let item: ClipboardItem = self
            .items
            .remove(id)
            .ok_or(ClipError::ItemNotFound(id.to_string()))?;

        self.item_order.retain(|item_id| item_id != id);
        Ok(item)
    }

    /// Get item for paste operation
    pub fn get_paste_operation(&self, id: &str, dest: PathBuf) -> ClipResult<PasteOperation> {
        let item: &ClipboardItem = self
            .items
            .get(id)
            .ok_or(ClipError::ItemNotFound(id.to_string()))?;

        PasteOperation::new(item, dest)
    }

    /// Get all clipboard items in order
    pub fn items(&self) -> Vec<&ClipboardItem> {
        self.item_order
            .iter()
            .filter_map(|id| self.items.get(id))
            .collect()
    }

    /// Check if clipboard is empty
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Get clipboard item count
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Clear all clipboard items
    pub fn clear(&mut self) {
        self.items.clear();
        self.item_order.clear();
    }

    /// Get clipboard statistics
    pub fn stats(&self) -> ClipboardStats {
        ClipboardStats::new(&self.items, self.created_at)
    }

    fn insert_item(&mut self, item: ClipboardItem) -> ClipResult<()> {
        // Check for duplicates
        if self
            .items
            .values()
            .any(|existing| existing.source_path == item.source_path)
        {
            return Err(ClipError::DuplicateItem(item.source_path));
        }

        // Enforce item limit
        while self.items.len() >= self.config.max_items {
            if let Some(oldest_id) = self.item_order.first().cloned() {
                self.remove_item(&oldest_id)?;
            }
        }

        let id = item.id.clone();
        self.items.insert(id.clone(), item);
        self.item_order.push(id);

        Ok(())
    }
}

impl Default for Clipboard {
    fn default() -> Self {
        Self::new(ClipboardConfig::default())
    }
}

#[derive(Debug, Clone)]
pub struct ClipboardStats {
    pub total_items: usize,
    pub copy_items: usize,
    pub move_items: usize,
    pub total_size: u64,
    pub created_at: Instant,
}

impl ClipboardStats {
    fn new(items: &HashMap<String, ClipboardItem>, created_at: Instant) -> Self {
        let mut copy_items: usize = 0;
        let mut move_items: usize = 0;
        let mut total_size: u64 = 0;

        for item in items.values() {
            match item.operation {
                ClipboardOperation::Copy => copy_items += 1,
                ClipboardOperation::Move => move_items += 1,
            }
            total_size += item.metadata.size;
        }

        Self {
            total_items: items.len(),
            copy_items,
            move_items,
            total_size,
            created_at,
        }
    }
}
