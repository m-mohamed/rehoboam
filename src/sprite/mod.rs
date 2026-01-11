//! Sprite integration for remote Claude Code agent execution
//!
//! This module enables running Claude Code agents inside Sprites sandboxes
//! (remote VMs) while maintaining real-time monitoring through Rehoboam.
//!
//! ## Status
//! - Event receiving: COMPLETE (HookEventForwarder + spawn_forwarder)
//! - Command sending: SCAFFOLDED (SpriteController, SpriteManager)

pub mod config;
pub mod controller;
pub mod forwarder;
pub mod manager;

// Re-export forwarder (used in main.rs when --enable-sprites is set)
#[allow(unused_imports)]
pub use forwarder::spawn_forwarder;

// Scaffolded for future use - suppress dead_code warnings
#[allow(unused_imports)]
pub use config::SpriteConfig;
#[allow(unused_imports)]
pub use controller::SpriteController;
#[allow(unused_imports)]
pub use forwarder::HookEventForwarder;
#[allow(unused_imports)]
pub use manager::{CheckpointRecord, SpriteManager, SpriteSession};
