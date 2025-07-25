# Debounce Utility Documentation

## Overview
The `debounce` module provides utilities for debouncing and throttling events, batching operations, and rate-limiting actions to improve performance and user experience in async applications.

## Features
- DebounceConfig: Configurable debounce settings for different event types
- Debouncer: Async event debouncer for user input and frequent operations
- Throttler: Rate-limiting utility for periodic actions
- EventBatcher: Batches multiple events into a single operation

## Usage
- Use `DebounceConfig` to configure debounce behavior for search, redraw, or file system events
- Use `Debouncer` to debounce async events and reduce redundant operations
- Use `Throttler` to limit the frequency of expensive actions
- Use `EventBatcher` to batch events for efficient processing
