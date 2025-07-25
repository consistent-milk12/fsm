# ObjectInfo: Filesystem Entry Metadata (`src/fs/object_info.rs`)

This module defines the `ObjectInfo` struct and related types for representing rich metadata about files and directories.

## Features
- **Cross-Platform**: Works on Linux, macOS, and Windows.
- **Async-Friendly**: Provides async constructors for fast and slow metadata loading.
- **Rich Metadata**: Includes path, name, extension, type, size, item count, modification time, and more.
- **Lightweight and Full Variants**: Supports both lightweight (`LightObjectInfo`) and full (`ObjectInfo`) representations for performance.
- **Integration**: Used by the cache, directory scanner, and UI components.

## Main Types
- `ObjectInfo`: Main struct with all metadata fields.
- `LightObjectInfo`: Lightweight struct for fast initial scans.
- `ObjectType`: Enum for file, directory, symlink, or custom types.

## Usage
Use `ObjectInfo::from_path_light` for fast scans, and `ObjectInfo::from_light_info` or `ObjectInfo::from_path` for full metadata. Supports sorting, display, and caching in the file manager.
