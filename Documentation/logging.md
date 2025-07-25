# Logging System (`src/logging.rs`)

This module provides structured, high-performance logging for the file manager using the `tracing` ecosystem.

## Features
- **Daily Rolling File Logs**: Logs are written to `logs/app-YYYY-MM-DD.log` with automatic rotation.
- **Structured Logging**: All logs include sequence number, log level, file, line, and module path for easy debugging.
- **Custom Formatter**: Compact log format: `[SEQ] LEVEL [file:line mod::path] message`.
- **Log Directory Management**: Ensures the `logs/` directory is clean and ready at startup.
- **Environment-based Filtering**: Log level can be controlled via the `RUST_LOG` environment variable (default: `debug`).
- **Integration with Tracing**: Uses `tracing_subscriber` and `tracing_appender` for async, non-blocking logging.

## Usage
Call `Logger::init_tracing()` once at the start of `main` to initialize logging. All modules can then use `tracing::{info, debug, warn, error}` macros for structured log output.
