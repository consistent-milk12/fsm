//! Handler Registry - REMOVED
//!
//! This component is no longer needed in the simplified architecture.
//! The ModularActionDispatcher handles all action routing directly.
//!
//! Key reasons for removal:
//! 1. Circular dependency with EventProcessor
//! 2. Duplicate functionality with ActionDispatcher
//! 3. Over-engineered for direct terminal event → action mapping
//! 4. Event/Action dual abstraction adds complexity
//!
//! The simplified flow is:
//! Terminal Events → Actions (via main.rs) → ModularActionDispatcher → Handlers
//!
//! This eliminates:
//! - Event abstraction layer
//! - Handler registry complexity  
//! - Event processor batching
//! - Multiple priority systems
//!
//! All handler registration and routing is now handled by ModularActionDispatcher.
