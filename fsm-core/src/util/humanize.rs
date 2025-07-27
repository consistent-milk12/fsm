//! src/util/humanize.rs

pub fn human_readable_size(size: u64) -> String {
    if size == 0 {
        return "0 B".to_string();
    }
    let units: [&'static str; 7] = ["B", "KB", "MB", "GB", "TB", "PB", "EB"];
    let mut size_f: f64 = size as f64;
    let mut unit_idx: usize = 0;

    while size_f >= 1024.0 && unit_idx < units.len() - 1 {
        size_f /= 1024.0;
        unit_idx += 1;
    }

    if unit_idx == 0 {
        format!("{} {}", size, units[unit_idx])
    } else {
        format!("{:.1} {}", size_f, units[unit_idx])
    }
}
