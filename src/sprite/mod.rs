//! Sprite integration for remote Claude Code agent execution
//!
//! This module enables running Claude Code agents inside Sprites sandboxes
//! (remote VMs) while maintaining real-time monitoring through Rehoboam.
//!
//! v1.5: Adds distributed sprite swarms (SpritePool) for parallel task execution.
//! Supports hybrid mode with local planner and remote sprite workers.

pub mod config;
pub mod controller;
pub mod forwarder;
pub mod manager;

// Re-exports
pub use manager::CheckpointRecord;

// v1.5: Sprite pool management
pub use manager::{
    HybridConfig, HybridSwarmStatus, SpritePool, SpritePoolConfig, SpriteWorker, SpriteWorkerStatus,
};
