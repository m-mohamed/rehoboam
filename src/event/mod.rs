pub mod input;
pub mod socket;

use serde::{Deserialize, Serialize};

/// Application events
#[derive(Debug)]
pub enum Event {
    /// Hook event from Claude Code (boxed to reduce enum size)
    Hook(Box<HookEvent>),
    /// Keyboard input
    Key(crossterm::event::KeyEvent),
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
}

impl HookEvent {
    /// Valid status values
    pub const VALID_STATUSES: [&'static str; 4] = ["idle", "working", "attention", "compacting"];

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
            return Err("invalid status: expected idle, working, attention, or compacting");
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
/// Event-specific:
/// - `user_prompt`: The prompt text (UserPromptSubmit)
/// - `reason`: Why stopping (Stop/SubagentStop/SessionEnd)
/// - `trigger`: "manual" or "auto" (PreCompact)
/// - `source`: "startup"/"resume"/"clear" (SessionStart)
/// - `message`: Notification message (Notification)
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct ClaudeHookInput {
    /// Unique session identifier
    pub session_id: String,
    /// Hook type that triggered this event
    pub hook_event_name: String,
    /// Path to conversation transcript file
    pub transcript_path: String,
    /// Current working directory
    pub cwd: String,

    // Tool events (optional)
    /// Tool name: Bash, Read, Write, Edit, Glob, Grep, etc.
    #[serde(default)]
    pub tool_name: Option<String>,
    /// Tool parameters as JSON object
    #[serde(default)]
    pub tool_input: Option<serde_json::Value>,
    /// Tool output (PostToolUse only)
    #[serde(default)]
    pub tool_response: Option<serde_json::Value>,
    /// Correlates PreToolUse→PostToolUse for latency tracking
    #[serde(default)]
    pub tool_use_id: Option<String>,

    // Event-specific fields
    /// User prompt text (UserPromptSubmit)
    #[serde(default)]
    pub user_prompt: Option<String>,
    /// Stop reason (Stop/SubagentStop/SessionEnd)
    #[serde(default)]
    pub reason: Option<String>,
    /// Session start source: startup, resume, clear (SessionStart)
    #[serde(default)]
    pub source: Option<String>,
    /// Compact trigger: manual, auto (PreCompact)
    #[serde(default)]
    pub trigger: Option<String>,
    /// Notification message (Notification)
    #[serde(default)]
    pub message: Option<String>,
}

impl ClaudeHookInput {
    /// Derive rehoboam status from Claude Code hook_event_name
    ///
    /// Maps Claude's hook types to our 4-state system:
    /// - idle: Session inactive
    /// - working: Claude processing
    /// - attention: Needs user input/permission
    /// - compacting: Context compaction in progress
    ///
    /// # Returns
    /// Tuple of (status, optional attention_type)
    pub fn derive_status(&self) -> (&str, Option<&str>) {
        match self.hook_event_name.as_str() {
            "SessionStart" => ("idle", None),
            "UserPromptSubmit" | "PreToolUse" | "PostToolUse" => ("working", None),
            "PermissionRequest" => ("attention", Some("permission")),
            "Notification" => ("attention", Some("notification")),
            "Stop" | "SessionEnd" => ("idle", None),
            "PreCompact" => ("compacting", None),
            "PostCompact" | "SubagentStart" | "SubagentStop" => ("working", None),
            _ => ("idle", None),
        }
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
        };
        assert_eq!(
            event.validate(),
            Err("invalid status: expected idle, working, attention, or compacting")
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
            };
            assert!(
                event.validate().is_ok(),
                "Status '{}' should be valid",
                status
            );
        }
    }
}
