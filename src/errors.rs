//! Structured error types for rehoboam
//!
//! Uses thiserror for ergonomic error definitions with automatic Display
//! and Error trait implementations.

use thiserror::Error;

/// All possible errors in rehoboam
#[derive(Error, Debug)]
pub enum RehoboamError {
    /// Hook installation failed
    #[error("Hook installation failed for '{project}': {reason}")]
    InitError { project: String, reason: String },

    /// Project discovery failed
    #[error("Project discovery failed: {0}")]
    DiscoveryError(String),
}
