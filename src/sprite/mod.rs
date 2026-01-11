//! Sprite integration for remote Claude Code agent execution
//!
//! This module enables running Claude Code agents inside Sprites sandboxes
//! (remote VMs) while maintaining real-time monitoring through Rehoboam.

pub mod config;
pub mod controller;
pub mod forwarder;
pub mod manager;

pub use config::SpriteConfig;
pub use controller::SpriteController;
pub use forwarder::HookEventForwarder;
pub use manager::{SpriteManager, SpriteSession};
