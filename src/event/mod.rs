pub mod input;
pub mod socket;

use serde::{Deserialize, Serialize};

/// Context window usage information from Claude Code 2.1.x
///
/// Tracks how much of the context window is being used, allowing
/// the TUI to display warnings when context is nearly full.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ContextWindow {
    /// Percentage of context used (0.0-100.0)
    #[serde(default)]
    pub used_percentage: Option<f64>,
    /// Total tokens in context
    #[serde(default)]
    pub total_tokens: Option<u64>,
}

/// Application events
#[derive(Debug)]
pub enum Event {
    /// Hook event from Claude Code (boxed to reduce enum size)
    Hook(Box<HookEvent>),
    /// Keyboard input
    Key(crossterm::event::KeyEvent),
    /// Remote hook event from a sprite
    RemoteHook {
        /// Sprite identifier
        sprite_id: String,
        /// The hook event
        event: Box<HookEvent>,
    },
    /// Sprite status change
    SpriteStatus {
        /// Sprite identifier
        sprite_id: String,
        /// New status
        status: SpriteStatusType,
    },
    /// Checkpoint data fetched from sprites API
    CheckpointData {
        /// Sprite identifier
        sprite_id: String,
        /// List of checkpoints
        checkpoints: Vec<sprites::Checkpoint>,
    },
}

/// Sprite status types
#[derive(Debug, Clone)]
pub enum SpriteStatusType {
    /// Sprite connected to WebSocket
    Connected,
    /// Sprite disconnected from WebSocket
    Disconnected,
}

/// Source of a hook event
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum EventSource {
    /// Event from local Unix socket (Claude Code on this machine)
    #[default]
    Local,
    /// Event from a remote sprite
    Sprite {
        /// Sprite identifier
        sprite_id: String,
    },
}

/// Event sent over Unix socket from hook handler to TUI
///
/// # Core Fields (required)
/// - `event`: Hook name (e.g., "PreToolUse", "Stop", "PermissionRequest")
/// - `status`: Agent status ("idle", "working", "attention", "compacting")
/// - `pane_id`: WezTerm pane ID where agent is running
/// - `project`: Git project name or directory name
/// - `timestamp`: Unix timestamp (seconds since epoch)
///
/// # v1.0 Rich Data Fields (optional)
/// - `session_id`: Claude Code session identifier
/// - `tool_name`: Current tool being used (Bash, Read, etc.)
/// - `tool_input`: Tool parameters as JSON
/// - `tool_use_id`: Correlates PreToolUse→PostToolUse for latency
///
/// # Validation
/// Use `validate()` to check that required fields are present and status is valid.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HookEvent {
    /// Hook name that triggered this event
    pub event: String,
    /// Agent status: idle, working, attention, or compacting
    pub status: String,
    /// Attention type (permission, notification) - only for attention status
    #[serde(default)]
    pub attention_type: Option<String>,
    /// WezTerm pane ID (unique identifier for the terminal pane)
    pub pane_id: String,
    /// Git project name or current directory name
    pub project: String,
    /// Unix timestamp in seconds since epoch
    pub timestamp: i64,

    // v1.0 rich data fields
    /// Claude Code session identifier
    #[serde(default)]
    pub session_id: Option<String>,
    /// Tool name: Bash, Read, Write, Edit, Glob, Grep, etc.
    #[serde(default)]
    pub tool_name: Option<String>,
    /// Tool parameters as JSON object
    #[serde(default)]
    pub tool_input: Option<serde_json::Value>,
    /// Correlates PreToolUse→PostToolUse for latency tracking
    #[serde(default)]
    pub tool_use_id: Option<String>,

    // v0.9.0 loop mode fields
    /// Stop reason (from Stop/SubagentStop hooks) - used for loop control
    #[serde(default)]
    pub reason: Option<String>,

    // v0.9.0 subagent fields
    /// Subagent session ID (SubagentStart/SubagentStop)
    #[serde(default)]
    pub subagent_id: Option<String>,
    /// Subagent description (SubagentStart)
    #[serde(default)]
    pub description: Option<String>,
    /// Subagent duration in milliseconds (SubagentStop)
    #[serde(default)]
    pub subagent_duration_ms: Option<u64>,

    // v0.10.0 sprite fields
    /// Event source: local or remote sprite
    #[serde(default)]
    pub source: EventSource,

    // Claude Code 2.1.x enriched fields
    /// Context window usage
    #[serde(default)]
    pub context_window: Option<ContextWindow>,
    /// Agent type from --agent flag (e.g., "explore", "plan")
    #[serde(default)]
    pub agent_type: Option<String>,
    /// Permission mode: "default", "plan", "acceptEdits", "dontAsk", "bypassPermissions"
    #[serde(default)]
    pub permission_mode: Option<String>,
    /// Current working directory
    #[serde(default)]
    pub cwd: Option<String>,
    /// Path to conversation transcript (.jsonl)
    #[serde(default)]
    pub transcript_path: Option<String>,
}

impl HookEvent {
    /// Valid status values (idle removed - now Attention with Waiting type)
    pub const VALID_STATUSES: [&'static str; 3] = ["working", "attention", "compacting"];

    /// Validate that required fields are present and status is valid
    ///
    /// # Returns
    /// - `Ok(())` if event is valid
    /// - `Err(&str)` with description of validation failure
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.pane_id.is_empty() {
            return Err("pane_id is required");
        }
        if self.project.is_empty() {
            return Err("project is required");
        }
        if !Self::VALID_STATUSES.contains(&self.status.as_str()) {
            return Err("invalid status: expected working, attention, or compacting");
        }
        Ok(())
    }
}

/// Raw input from Claude Code hooks (stdin JSON)
///
/// Claude Code pipes this JSON to hook commands via stdin. This struct captures
/// ALL available fields for rich observability.
///
/// # Fields
/// Common to all hooks:
/// - `session_id`: Unique session identifier
/// - `hook_event_name`: Hook type (SessionStart, PreToolUse, etc.)
/// - `transcript_path`: Path to .jsonl conversation file
/// - `cwd`: Current working directory
///
/// Tool hooks (PreToolUse/PostToolUse):
/// - `tool_name`: Bash, Read, Write, Edit, Glob, etc.
/// - `tool_input`: Tool parameters as JSON
/// - `tool_use_id`: Correlates Pre→Post events for latency calculation
/// - `tool_response`: Tool output (PostToolUse only)
///
/// Claude Code hook input structure
///
/// Only includes fields we actively use. Serde ignores unknown fields by default.
#[derive(Debug, Clone, Deserialize)]
pub struct ClaudeHookInput {
    /// Unique session identifier
    pub session_id: String,
    /// Hook type that triggered this event
    pub hook_event_name: String,

    // Tool events (optional)
    /// Tool name: Bash, Read, Write, Edit, Glob, Grep, etc.
    #[serde(default)]
    pub tool_name: Option<String>,
    /// Tool parameters as JSON object
    #[serde(default)]
    pub tool_input: Option<serde_json::Value>,
    /// Correlates PreToolUse→PostToolUse for latency tracking
    #[serde(default)]
    pub tool_use_id: Option<String>,

    // Event-specific fields
    /// Stop reason (Stop/SubagentStop/SessionEnd)
    #[serde(default)]
    pub reason: Option<String>,
    /// Notification message (Notification)
    #[serde(default)]
    pub message: Option<String>,

    // Subagent fields
    /// Subagent session ID (SubagentStart/SubagentStop)
    #[serde(default)]
    pub subagent_id: Option<String>,
    /// Subagent description (SubagentStart)
    #[serde(default)]
    pub description: Option<String>,
    /// Subagent duration in milliseconds (SubagentStop)
    #[serde(default)]
    pub duration_ms: Option<u64>,

    // Claude Code 2.1.x fields
    /// Context window usage percentage (0.0-100.0)
    #[serde(default)]
    pub context_window: Option<ContextWindow>,
    /// Agent type from --agent flag (e.g., "explore", "plan")
    #[serde(default)]
    pub agent_type: Option<String>,
    /// Permission mode: "default", "plan", "acceptEdits", "dontAsk", "bypassPermissions"
    #[serde(default)]
    pub permission_mode: Option<String>,
    /// Current working directory
    #[serde(default)]
    pub cwd: Option<String>,
    /// Path to conversation transcript (.jsonl)
    #[serde(default)]
    pub transcript_path: Option<String>,
}

impl ClaudeHookInput {
    /// Derive rehoboam status from Claude Code hook_event_name
    ///
    /// Maps Claude's hook types to our 3-state system:
    /// - working: Claude actively processing (tools, thinking)
    /// - attention: Claude needs user attention (permission, input, notification, waiting)
    /// - compacting: Context compaction in progress
    ///
    /// Attention sub-types:
    /// - permission: Tool needs explicit approval (blocking)
    /// - notification: Informational alert
    /// - waiting: Ready for new prompt (was "idle")
    ///
    /// # Returns
    /// Tuple of (status, optional attention_type)
    pub fn derive_status(&self) -> (&str, Option<&str>) {
        match self.hook_event_name.as_str() {
            // WORKING: Claude is actively doing something
            "UserPromptSubmit" => ("working", None), // User sent message, Claude processing
            "PreToolUse" => ("working", None),       // About to run a tool
            "PostToolUse" => ("working", None),      // Just finished a tool, may continue
            "SubagentStart" => ("working", None),    // Spawned a subagent
            "SubagentStop" => ("working", None),     // Subagent finished, may continue
            "Setup" => ("working", None),            // Claude Code 2.1.x: initialization/setup phase

            // ATTENTION: Claude needs user attention
            "PermissionRequest" => ("attention", Some("permission")),

            // ATTENTION(Waiting): Claude is waiting for user input (was idle)
            "SessionStart" => ("attention", Some("waiting")), // Session started, waiting for prompt
            "Stop" => ("attention", Some("waiting")),         // Claude finished responding
            "SessionEnd" => ("attention", Some("waiting")),   // Session ended
            "Notification" => ("attention", Some("notification")), // Informational message

            // COMPACTING: Context maintenance
            "PreCompact" => ("compacting", None),

            // Unknown hooks default to attention(waiting)
            _ => ("attention", Some("waiting")),
        }
    }
}

/// Derive rehoboam status from a hook event name string
///
/// Standalone function for use by remote event processing (sprites)
/// where we only have the hook event name as a string.
///
/// # Returns
/// Tuple of (status, optional attention_type) as owned Strings
pub fn derive_status_from_hook_name(hook_name: &str) -> (String, Option<String>) {
    match hook_name {
        // WORKING: Claude is actively doing something
        "UserPromptSubmit" | "PreToolUse" | "PostToolUse" | "SubagentStart" | "SubagentStop"
        | "Setup" => ("working".to_string(), None),

        // ATTENTION: Claude is BLOCKED waiting for explicit user approval
        "PermissionRequest" => ("attention".to_string(), Some("permission".to_string())),

        // ATTENTION(Waiting): Claude is waiting for user input (was "idle")
        "SessionStart" | "Stop" | "SessionEnd" => {
            ("attention".to_string(), Some("waiting".to_string()))
        }

        // ATTENTION(Notification): Informational alert
        "Notification" => ("attention".to_string(), Some("notification".to_string())),

        // COMPACTING: Context maintenance
        "PreCompact" => ("compacting".to_string(), None),

        // Unknown hooks default to attention(waiting) (conservative)
        _ => ("attention".to_string(), Some("waiting".to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hook_event_deserialize_valid() {
        let json = r#"{
            "event": "PreToolUse",
            "status": "working",
            "pane_id": "42",
            "project": "my-project",
            "timestamp": 1704067200
        }"#;

        let event: HookEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.event, "PreToolUse");
        assert_eq!(event.status, "working");
        assert_eq!(event.pane_id, "42");
        assert_eq!(event.project, "my-project");
        assert!(event.attention_type.is_none());
        assert!(event.validate().is_ok());
    }

    #[test]
    fn test_hook_event_deserialize_with_attention_type() {
        let json = r#"{
            "event": "PermissionRequest",
            "status": "attention",
            "attention_type": "permission",
            "pane_id": "42",
            "project": "my-project",
            "timestamp": 1704067200
        }"#;

        let event: HookEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.status, "attention");
        assert_eq!(event.attention_type, Some("permission".to_string()));
        assert!(event.validate().is_ok());
    }

    #[test]
    fn test_hook_event_validate_empty_pane_id() {
        let event = HookEvent {
            event: "Test".to_string(),
            status: "working".to_string(),
            attention_type: None,
            pane_id: "".to_string(),
            project: "test".to_string(),
            timestamp: 0,
            session_id: None,
            tool_name: None,
            tool_input: None,
            tool_use_id: None,
            reason: None,
            subagent_id: None,
            description: None,
            subagent_duration_ms: None,
            source: EventSource::Local,
            context_window: None,
            agent_type: None,
            permission_mode: None,
            cwd: None,
            transcript_path: None,
        };
        assert_eq!(event.validate(), Err("pane_id is required"));
    }

    #[test]
    fn test_hook_event_validate_empty_project() {
        let event = HookEvent {
            event: "Test".to_string(),
            status: "working".to_string(),
            attention_type: None,
            pane_id: "42".to_string(),
            project: "".to_string(),
            timestamp: 0,
            session_id: None,
            tool_name: None,
            tool_input: None,
            tool_use_id: None,
            reason: None,
            subagent_id: None,
            description: None,
            subagent_duration_ms: None,
            source: EventSource::Local,
            context_window: None,
            agent_type: None,
            permission_mode: None,
            cwd: None,
            transcript_path: None,
        };
        assert_eq!(event.validate(), Err("project is required"));
    }

    #[test]
    fn test_hook_event_validate_invalid_status() {
        let event = HookEvent {
            event: "Test".to_string(),
            status: "invalid".to_string(),
            attention_type: None,
            pane_id: "42".to_string(),
            project: "test".to_string(),
            timestamp: 0,
            session_id: None,
            tool_name: None,
            tool_input: None,
            tool_use_id: None,
            reason: None,
            subagent_id: None,
            description: None,
            subagent_duration_ms: None,
            source: EventSource::Local,
            context_window: None,
            agent_type: None,
            permission_mode: None,
            cwd: None,
            transcript_path: None,
        };
        assert_eq!(
            event.validate(),
            Err("invalid status: expected working, attention, or compacting")
        );
    }

    #[test]
    fn test_hook_event_all_valid_statuses() {
        for status in HookEvent::VALID_STATUSES {
            let event = HookEvent {
                event: "Test".to_string(),
                status: status.to_string(),
                attention_type: None,
                pane_id: "42".to_string(),
                project: "test".to_string(),
                timestamp: 0,
                session_id: None,
                tool_name: None,
                tool_input: None,
                tool_use_id: None,
                reason: None,
                subagent_id: None,
                description: None,
                subagent_duration_ms: None,
                source: EventSource::Local,
                context_window: None,
                agent_type: None,
                permission_mode: None,
                cwd: None,
                transcript_path: None,
            };
            assert!(
                event.validate().is_ok(),
                "Status '{}' should be valid",
                status
            );
        }
    }
}
