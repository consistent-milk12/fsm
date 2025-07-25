# FSM Implementation Specification

**ACTIVE FEATURE:** Phase 2.3: FileOperationsOverlay UI Component

## 1. Executive Summary
**Objective:** Implement visual progress indicators for file operations  
**Priority:** High (blocks user experience for large file operations)  
**Complexity:** Medium (UI integration + real-time updates)  
**Dependencies:** Phase 2.1 & 2.2 progress backend (✅ Complete)  
**Estimated Effort:** 1-2 development sessions  

## 2. Context & Background
**Problem:** Users have no visual feedback during large file operations  
**Current State:** Backend progress tracking complete via UIState.active_file_operations HashMap  
**Required:** UI component to visualize real-time progress data  
**Integration Point:** Main UI rendering pipeline with conditional display  

## 3. Success Criteria
### Must Have (P0)
- [ ] **Real-time Updates**: Progress bars reflect current operation state
- [ ] **Multi-operation Support**: Handle concurrent operations with separate bars  
- [ ] **Performance Metrics**: Display throughput (MB/s) and ETA
- [ ] **File Context**: Show current file being processed
- [ ] **Auto-hide**: Overlay appears/disappears with active operations

### Should Have (P1)
- [ ] **Color Coding**: Blue=Copy, Yellow=Move, Green=Rename
- [ ] **Responsive Layout**: Adapt to terminal size, max 1/3 screen height
- [ ] **Cancel Instruction**: Clear ESC key instruction displayed
- [ ] **Non-intrusive**: Positioned at bottom, doesn't cover main content

### Could Have (P2)
- [ ] **Animation**: Smooth progress bar transitions
- [ ] **Sound Feedback**: Optional completion notifications

## 4. Technical Approach
**Architecture:** Component-based UI with ratatui Gauge widgets  
**Data Flow:** UIState.active_file_operations → FileOperationsOverlay → Terminal  
**Performance:** Conditional rendering only when operations active  
**Error Handling:** Graceful degradation for edge cases

## Implementation Specification

### 1. Core Component Structure
```rust
// src/view/components/file_operations_overlay.rs (NEW FILE)
use crate::model::ui_state::FileOperationProgress;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    widgets::{Block, Borders, Gauge, Paragraph},
    Frame,
};
use std::collections::HashMap;

pub struct FileOperationsOverlay;

impl FileOperationsOverlay {
    /// Main render function - entry point for overlay display
    pub fn render(f: &mut Frame, area: Rect, operations: &HashMap<String, FileOperationProgress>) {
        if operations.is_empty() { return; }
        
        let areas = Self::calculate_layout(area, operations.len());
        
        for (i, (_id, progress)) in operations.iter().enumerate() {
            if let Some(op_area) = areas.get(i) {
                Self::render_single_operation(f, *op_area, progress);
            }
        }
        
        Self::render_cancel_instruction(f, &areas);
    }
    
    /// Render individual progress bar with metrics
    fn render_single_operation(f: &mut Frame, area: Rect, progress: &FileOperationProgress) {
        let percentage = (progress.progress_ratio() * 100.0) as u16;
        let throughput = Self::format_throughput(progress.throughput_bps);
        let eta = Self::format_eta(progress.estimated_completion);
        let file_display = Self::truncate_path(&progress.current_file, 35);
        let file_count = format!("({}/{})", progress.files_completed, progress.total_files);
        
        let color = match progress.operation_type.as_str() {
            "Copy" => Color::Blue,
            "Move" => Color::Yellow, 
            "Rename" => Color::Green,
            _ => Color::Cyan,
        };
        
        let gauge = Gauge::default()
            .block(Block::default().borders(Borders::ALL)
                .title(format!("{} {} {}", progress.operation_type, file_display, file_count)))
            .gauge_style(Style::default().fg(color))
            .percent(percentage)
            .label(format!("{}% ({}, {})", percentage, throughput, eta));
            
        f.render_widget(gauge, area);
    }
}
```

### 2. Layout & Display Logic
```rust
impl FileOperationsOverlay {
    /// Calculate dynamic layout based on operation count
    fn calculate_layout(area: Rect, operation_count: usize) -> Vec<Rect> {
        let available_height = area.height.saturating_sub(1); // Reserve for instruction
        let op_height = std::cmp::max(3, available_height / operation_count as u16);
        
        (0..operation_count).map(|i| Rect {
            x: area.x,
            y: area.y + (i as u16 * op_height),
            width: area.width,
            height: op_height,
        }).collect()
    }
    
    /// Display cancel instruction at bottom
    fn render_cancel_instruction(f: &mut Frame, areas: &[Rect]) {
        if let Some(last_area) = areas.last() {
            let instruction_area = Rect {
                y: last_area.y + last_area.height,
                height: 1,
                ..*last_area
            };
            
            let text = Paragraph::new("Press ESC to cancel operations")
                .style(Style::default().fg(Color::Gray));
            f.render_widget(text, instruction_area);
        }
    }
    
    /// Format throughput for display
    fn format_throughput(bps: Option<u64>) -> String {
        match bps {
            Some(bytes) => {
                let (size, unit) = Self::scale_bytes(bytes);
                format!("{:.1}{}/s", size, unit)
            }
            None => "calculating...".to_string(),
        }
    }
    
    /// Format ETA for display  
    fn format_eta(eta: Option<std::time::Instant>) -> String {
        match eta {
            Some(time) => {
                if let Ok(remaining) = time.duration_since(std::time::Instant::now()) {
                    format!("{}s remaining", remaining.as_secs())
                } else {
                    "finishing...".to_string()
                }
            }
            None => "calculating...".to_string(),
        }
    }
    
    /// Scale bytes to appropriate unit
    fn scale_bytes(bytes: u64) -> (f64, &'static str) {
        const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
        let mut size = bytes as f64;
        let mut unit_idx = 0;
        
        while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
            size /= 1024.0;
            unit_idx += 1;
        }
        
        (size, UNITS[unit_idx])
    }
    
    /// Truncate path for display
    fn truncate_path(path: &std::path::Path, max_len: usize) -> String {
        let path_str = path.to_string_lossy();
        if path_str.len() <= max_len {
            path_str.to_string()
        } else {
            format!("...{}", &path_str[path_str.len() - max_len + 3..])
        }
    }
}
```

### 3. Main UI Integration
```rust
// src/view/ui.rs - Add to main render function
use crate::view::components::file_operations_overlay::FileOperationsOverlay;

pub fn render(f: &mut Frame, app: &AppState) {
    // ... existing UI rendering ...
    
    // Render progress overlay if operations are active
    if !app.ui.active_file_operations.is_empty() {
        let overlay_area = calculate_progress_overlay_area(f.size(), app.ui.active_file_operations.len());
        FileOperationsOverlay::render(f, overlay_area, &app.ui.active_file_operations);
    }
}

/// Calculate overlay position - bottom of screen, above status bar
fn calculate_progress_overlay_area(screen_size: Rect, operation_count: usize) -> Rect {
    let overlay_height = std::cmp::min(
        (operation_count * 3 + 2) as u16,  // 3 lines per operation + instruction
        screen_size.height / 3             // Maximum 1/3 of screen
    );
    
    Rect {
        x: 1,
        y: screen_size.height.saturating_sub(overlay_height + 2), // Above status bar
        width: screen_size.width.saturating_sub(2),
        height: overlay_height,
    }
}
```

### 4. Module Export Setup
```rust
// src/view/components/mod.rs - Add new component
pub mod file_operations_overlay;
pub use file_operations_overlay::FileOperationsOverlay;
```

## Success Criteria
- [ ] **Real-time Updates**: Progress bars reflect current operation state
- [ ] **Multi-operation Support**: Handle concurrent operations with separate bars
- [ ] **Performance Metrics**: Display throughput (MB/s) and ETA
- [ ] **File Context**: Show current file being processed
- [ ] **Color Coding**: Blue=Copy, Yellow=Move, Green=Rename
- [ ] **Responsive Layout**: Adapt to terminal size, max 1/3 screen height
- [ ] **Auto-hide**: Overlay appears/disappears with active operations
- [ ] **Non-intrusive**: Positioned at bottom, doesn't cover main content
- [ ] **Cancel Instruction**: Clear ESC key instruction displayed

## Technical Requirements
- **Performance**: Conditional rendering only when operations active
- **Memory**: Efficient string handling for path truncation
- **Layout**: Dynamic height calculation based on operation count
- **Integration**: Seamless addition to existing UI pipeline
- **Error Handling**: Graceful degradation for edge cases

## Testing Approach
1. Start large file copy operation
2. Verify progress bar appears with correct metrics
3. Test multiple concurrent operations
4. Validate color coding and file display
5. Confirm overlay disappears on completion
6. Test responsive behavior with terminal resize

## 5. Testing Strategy
**Unit Tests:** Component rendering with mock progress data  
**Integration Tests:** Full UI pipeline with real file operations  
**Performance Tests:** Memory usage with multiple concurrent operations  
**User Acceptance:** Manual testing with large file operations  

## 6. Risk Assessment
**High Risk:** Real-time UI updates causing performance degradation  
**Medium Risk:** Terminal resize handling during active operations  
**Low Risk:** Color coding not displaying correctly on all terminals  

**Mitigation:** Conditional rendering, responsive layout calculations, fallback colors

## 7. Rollback Plan
**Failure Condition:** UI performance degradation or rendering issues  
**Rollback Steps:**
1. Remove FileOperationsOverlay from UI rendering
2. Revert lib.rs module declaration  
3. Delete overlay component file
4. Operations continue with backend-only progress tracking

## 8. Definition of Done
- [ ] All P0 success criteria met and tested
- [ ] Code passes cargo fmt + check + clippy + build
- [ ] Manual testing completed across different terminal sizes
- [ ] Integration with existing UI verified
- [ ] Documentation updated in Design.md
- [ ] Next phase specification prepared

---

**This completes the visual layer for the robust progress tracking system built in Phases 2.1 & 2.2.**

---

## Implementation Template (For Future Phases)

```markdown
# FSM Implementation Specification

**ACTIVE FEATURE:** Phase X.X: [Feature Name]

## 1. Executive Summary
**Objective:** [Clear objective statement]
**Priority:** [High/Medium/Low + business justification]
**Complexity:** [Low/Medium/High + technical factors]
**Dependencies:** [List with completion status]
**Estimated Effort:** [Time estimate]

## 2. Context & Background
**Problem:** [What problem are we solving]
**Current State:** [What exists today]
**Required:** [What needs to be built]
**Integration Point:** [Where this fits in the system]

## 3. Success Criteria
### Must Have (P0)
- [ ] [Critical functionality]

### Should Have (P1)  
- [ ] [Important but not blocking]

### Could Have (P2)
- [ ] [Nice to have features]

## 4. Technical Approach
**Architecture:** [High-level design approach]
**Data Flow:** [How data moves through the system]
**Performance:** [Performance considerations]
**Error Handling:** [Error handling strategy]

## 5. Testing Strategy
[Testing approach and requirements]

## 6. Risk Assessment
[Risks and mitigation strategies]

## 7. Rollback Plan
[How to rollback if things go wrong]

## 8. Definition of Done
[Checklist for completion]
```