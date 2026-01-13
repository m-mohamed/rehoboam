// Clippy configuration
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::cast_possible_wrap)] // Timestamp u64->i64 won't overflow until year 292 billion
#![allow(clippy::option_if_let_else)] // if-let is more readable in error handling
#![allow(clippy::single_match_else)] // match is fine for Result handling
#![allow(clippy::doc_markdown)] // Don't require backticks in doc comments
#![allow(clippy::manual_let_else)] // if-let is clearer for multi-line error handling

//! Rehoboam Bridge - Hook event forwarder for remote sprites
//!
//! A lightweight binary that runs inside Sprite VMs and forwards
//! Claude Code hook events to the Rehoboam TUI via WebSocket.
//!
//! Usage (inside sprite, called by Claude Code hooks):
//!   rehoboam-bridge
//!
//! Environment variables:
//!   `REHOBOAM_HOST` - Rehoboam server address (required, e.g., "192.168.1.100:9876")
//!   `SPRITE_ID`     - Unique identifier for this sprite (required)
//!
//! The bridge reads hook JSON from stdin (same format as local hooks),
//! wraps it with sprite metadata, and sends via WebSocket.

use futures_util::SinkExt;
use serde::{Deserialize, Serialize};
use std::io::{self, BufRead};
use tokio_tungstenite::{connect_async, tungstenite::Message};

/// Remote hook event sent to Rehoboam
#[derive(Debug, Serialize)]
struct RemoteHookEvent {
    /// Sprite identifier
    sprite_id: String,

    /// Original hook event data
    event: HookEventData,

    /// Timestamp when event was sent
    timestamp: i64,
}

/// Hook event data from Claude Code
#[derive(Debug, Clone, Serialize, Deserialize)]
struct HookEventData {
    /// Session ID
    #[serde(default)]
    session_id: Option<String>,

    /// Hook event name (`PreToolUse`, `PostToolUse`, etc.)
    #[serde(default)]
    hook_event_name: Option<String>,

    /// Tool name (for tool events)
    #[serde(default)]
    tool_name: Option<String>,

    /// Tool input (for tool events)
    #[serde(default)]
    tool_input: Option<serde_json::Value>,

    /// Transcript path
    #[serde(default)]
    transcript_path: Option<String>,

    /// Stop reason (Stop events)
    #[serde(default)]
    reason: Option<String>,

    /// Working directory
    #[serde(default)]
    cwd: Option<String>,
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    // Read required environment variables
    let rehoboam_host = if let Ok(host) = std::env::var("REHOBOAM_HOST") {
        host
    } else {
        eprintln!("Error: REHOBOAM_HOST environment variable not set");
        eprintln!("Set it to the Rehoboam server address, e.g., REHOBOAM_HOST=192.168.1.100:9876");
        std::process::exit(1);
    };

    let sprite_id = if let Ok(id) = std::env::var("SPRITE_ID") {
        id
    } else {
        eprintln!("Error: SPRITE_ID environment variable not set");
        eprintln!("Set it to a unique identifier for this sprite");
        std::process::exit(1);
    };

    // Read JSON from stdin (Claude Code pipes it)
    let stdin = io::stdin();
    let mut input = String::new();
    for line in stdin.lock().lines().map_while(Result::ok) {
        input.push_str(&line);
    }

    if input.trim().is_empty() {
        // Silent exit - no input
        return;
    }

    // Parse the hook JSON
    let hook_data: HookEventData = match serde_json::from_str(&input) {
        Ok(parsed) => parsed,
        Err(e) => {
            eprintln!("Failed to parse hook JSON: {e}");
            std::process::exit(1);
        }
    };

    // Get current timestamp
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    // Build remote event
    let remote_event = RemoteHookEvent {
        sprite_id,
        event: hook_data,
        timestamp,
    };

    // Connect to Rehoboam WebSocket server
    let ws_url = format!("ws://{rehoboam_host}");
    let (mut ws_stream, _) = match connect_async(&ws_url).await {
        Ok(conn) => conn,
        Err(e) => {
            // Silent failure - Rehoboam may not be running
            eprintln!("WebSocket connection failed: {e}");
            return;
        }
    };

    // Send the event
    let json = match serde_json::to_string(&remote_event) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("Failed to serialize event: {e}");
            return;
        }
    };

    if let Err(e) = ws_stream.send(Message::Text(json.into())).await {
        eprintln!("Failed to send event: {e}");
    }

    // Close connection cleanly
    let _ = ws_stream.close(None).await;
}
