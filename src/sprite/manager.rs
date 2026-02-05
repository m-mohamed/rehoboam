//! Sprite checkpoint management
//!
//! # Components
//!
//! - [`CheckpointRecord`] - Used for checkpoint timeline display in the TUI

/// A checkpoint in the timeline
#[derive(Debug, Clone)]
pub struct CheckpointRecord {
    /// Checkpoint ID from Sprites API
    pub id: String,

    /// User-provided comment/description
    pub comment: String,

    /// When the checkpoint was created (Unix timestamp in seconds)
    pub created_at: i64,
}

impl From<sprites::Checkpoint> for CheckpointRecord {
    fn from(cp: sprites::Checkpoint) -> Self {
        Self {
            id: cp.id,
            comment: cp.comment.unwrap_or_default(),
            created_at: cp.created_at.map(|dt| dt.timestamp()).unwrap_or(0),
        }
    }
}
