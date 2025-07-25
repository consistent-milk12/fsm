# Error Handling (`src/error.rs`)

This module defines the unified error type `AppError` for the entire file manager application. All major modules use `Result<T, AppError>` for consistent error propagation and diagnostics.

## Features
- **Comprehensive Error Enum**: `AppError` covers I/O, config, cache, search, file operations, UI, plugin, and more.
- **Rich Context**: Many variants carry extra context (e.g., file path, operation, error source).
- **Automatic Conversion**: Implements `From` for common error types (e.g., `io::Error`, `toml::de::Error`, `serde_json::Error`).
- **Manual Clone**: Custom `Clone` implementation to handle non-Clone fields.
- **Helper Constructors**: Methods for common error creation patterns (e.g., `search_failed`, `file_operation_failed`).

## Main Variants
- `Io`, `FsMetadata`, `PermissionDenied`, `NotFound`: Filesystem and I/O errors.
- `Cache`, `CacheOperation`: Caching layer errors.
- `Config`, `ConfigIo`: Configuration file parsing and I/O errors.
- `Serde`: Serialization/deserialization errors.
- `ExternalCmd`, `Ripgrep`: External process/command failures.
- `SearchFailed`, `FileOperationFailed`, `NavigationFailed`: Domain-specific errors.
- `UiComponent`, `InvalidInput`, `TaskFailed`, `TaskTimeout`: UI and task management errors.
- `Cancelled`, `Terminal`, `Resize`, `Plugin`, `Other`: Miscellaneous and fallback errors.

## Usage
All public APIs and background tasks should return `Result<T, AppError>`. Use the provided helper methods to attach context or create specific error variants as needed.
