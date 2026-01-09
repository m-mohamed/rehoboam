//! Structured error types for rehoboam
//!
//! Uses thiserror for ergonomic error definitions with automatic Display
//! and Error trait implementations.

use thiserror::Error;

/// All possible errors in rehoboam
#[allow(dead_code)]
#[derive(Error, Debug)]
pub enum RehoboamError {
    /// Invalid status value in hook event
    #[error("Invalid status '{0}'. Expected: idle, working, attention, compacting")]
    InvalidStatus(String),

    /// Required field missing in hook event
    #[error("Missing required field: {0}")]
    MissingField(&'static str),

    /// Socket connection or I/O error
    #[error("Socket error: {0}")]
    SocketError(#[from] std::io::Error),

    /// JSON parsing/serialization error
    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),

    /// Hook installation failed
    #[error("Hook installation failed for '{project}': {reason}")]
    InitError { project: String, reason: String },

    /// Project discovery failed
    #[error("Project discovery failed: {0}")]
    DiscoveryError(String),

    /// Terminal setup or restoration error
    #[error("Terminal error: {0}")]
    TerminalError(String),

    /// Event validation failed
    #[error("Event validation failed: {0}")]
    ValidationError(&'static str),
}

/// Convenience Result type using RehoboamError
#[allow(dead_code)]
pub type Result<T> = std::result::Result<T, RehoboamError>;

impl From<&'static str> for RehoboamError {
    fn from(s: &'static str) -> Self {
        RehoboamError::ValidationError(s)
    }
}
