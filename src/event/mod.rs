pub mod input;
pub mod socket;
pub mod status;

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
    /// Percentage of context remaining (0.0-100.0) - Claude Code 2.1.6+
    /// Cleaner for threshold checks: `remaining < 20%` vs `used > 80%`
    #[serde(default)]
    pub remaining_percentage: Option<f64>,
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

    /// Stop reason (from Stop/SubagentStop hooks)
    #[serde(default)]
    pub reason: Option<String>,

    // v0.9.15 high-value fields from official schema
    /// Notification type: permission_prompt, idle_prompt, auth_success, elicitation_dialog
    #[serde(default)]
    pub notification_type: Option<String>,
    /// Notification title
    #[serde(default)]
    pub notification_title: Option<String>,
    /// Error message from PostToolUseFailure
    #[serde(default)]
    pub error: Option<String>,
    /// Whether tool failure was a user interrupt
    #[serde(default)]
    pub is_interrupt: Option<bool>,
    /// User's prompt text (UserPromptSubmit)
    #[serde(default)]
    pub prompt: Option<String>,

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

    // TeammateTool env vars (v3.0) - captured from environment at hook time
    /// Team name from CLAUDE_CODE_TEAM_NAME env var
    #[serde(default)]
    pub team_name: Option<String>,
    /// Agent ID from CLAUDE_CODE_AGENT_ID env var
    #[serde(default)]
    pub team_agent_id: Option<String>,
    /// Agent name from CLAUDE_CODE_AGENT_NAME env var
    #[serde(default)]
    pub team_agent_name: Option<String>,
    /// Agent type from CLAUDE_CODE_AGENT_TYPE env var
    #[serde(default)]
    pub team_agent_type: Option<String>,

    // Claude Code version tracking
    /// Claude Code version from CLAUDE_CODE_VERSION env var
    #[serde(default)]
    pub claude_code_version: Option<String>,

    /// Claude model used for this session (e.g., "claude-opus-4-5-20251101")
    #[serde(default)]
    pub model: Option<String>,

    // Claude Code 2.1.33 fields
    /// Session source: "startup", "resume", "clear", "compact" (SessionStart)
    #[serde(default)]
    pub session_source: Option<String>,
    /// Whether Claude continues due to stop hook (Stop/SubagentStop)
    #[serde(default)]
    pub stop_hook_active: Option<bool>,
    /// Subagent's own transcript path (SubagentStop)
    #[serde(default)]
    pub agent_transcript_path: Option<String>,
    /// What triggered compaction (PreCompact)
    #[serde(default)]
    pub trigger: Option<String>,

    /// Effort level from CLAUDE_CODE_EFFORT_LEVEL env var (e.g., "low", "medium", "high")
    #[serde(default)]
    pub effort_level: Option<String>,

    // TeammateIdle / TaskCompleted fields (Claude Code 2.1.33+)
    /// Teammate name (TeammateIdle/TaskCompleted)
    #[serde(default)]
    pub teammate_name: Option<String>,
    /// Task ID (TaskCompleted)
    #[serde(default)]
    pub task_id: Option<String>,
    /// Task subject (TaskCompleted)
    #[serde(default)]
    pub task_subject: Option<String>,
    /// Task description (TaskCompleted)
    #[serde(default)]
    pub task_description: Option<String>,

    // PostToolUse response
    /// Tool response output (PostToolUse)
    #[serde(default)]
    pub tool_response: Option<serde_json::Value>,
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
    /// Notification type: permission_prompt, idle_prompt, auth_success, elicitation_dialog
    #[serde(default)]
    pub notification_type: Option<String>,
    /// Notification title (Notification)
    #[serde(default)]
    pub title: Option<String>,
    /// Error message (PostToolUseFailure)
    #[serde(default)]
    pub error: Option<String>,
    /// Whether failure was a user interrupt (PostToolUseFailure)
    #[serde(default)]
    pub is_interrupt: Option<bool>,
    /// User's prompt text (UserPromptSubmit)
    #[serde(default)]
    pub prompt: Option<String>,

    // Subagent fields
    /// Subagent session ID (SubagentStart/SubagentStop)
    /// Claude Code sends `agent_id`; we alias it for backward compat
    #[serde(default, alias = "agent_id")]
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

    /// Claude model used for this session (e.g., "claude-opus-4-5-20251101")
    #[serde(default)]
    pub model: Option<String>,

    // Claude Code 2.1.33 fields
    /// Session source: "startup", "resume", "clear", "compact" (SessionStart)
    #[serde(default, rename = "source")]
    pub session_source: Option<String>,
    /// Whether Claude continues due to stop hook (Stop/SubagentStop)
    #[serde(default)]
    pub stop_hook_active: Option<bool>,
    /// Subagent's own transcript path (SubagentStop)
    #[serde(default)]
    pub agent_transcript_path: Option<String>,
    /// "Always allow" permission suggestions (PermissionRequest)
    /// Parsed for completeness; not yet forwarded to HookEvent.
    #[serde(default)]
    #[allow(dead_code)]
    pub permission_suggestions: Option<serde_json::Value>,
    /// What triggered compaction (PreCompact)
    #[serde(default)]
    pub trigger: Option<String>,

    // TeammateIdle / TaskCompleted fields (Claude Code 2.1.33+)
    /// Team name from JSON input (fallback when env var is absent)
    #[serde(default)]
    pub team_name: Option<String>,
    /// Teammate name (TeammateIdle/TaskCompleted)
    #[serde(default)]
    pub teammate_name: Option<String>,
    /// Task ID (TaskCompleted)
    #[serde(default)]
    pub task_id: Option<String>,
    /// Task subject (TaskCompleted)
    #[serde(default)]
    pub task_subject: Option<String>,
    /// Task description (TaskCompleted)
    #[serde(default)]
    pub task_description: Option<String>,

    // PostToolUse response
    /// Tool response output (PostToolUse)
    #[serde(default)]
    pub tool_response: Option<serde_json::Value>,
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
    pub fn derive_status(&self) -> (&'static str, Option<&'static str>) {
        status::derive_status_from_event(&self.hook_event_name)
    }
}

// NOTE: derive_status functions are now in event/status.rs
// Use derive_status_owned() for owned strings or derive_status_from_event() for references

/// Backward compatibility alias for derive_status_owned
#[inline]
pub fn derive_status_from_hook_name(hook_name: &str) -> (String, Option<String>) {
    status::derive_status_owned(hook_name)
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
            notification_type: None,
            notification_title: None,
            error: None,
            is_interrupt: None,
            prompt: None,
            subagent_id: None,
            description: None,
            subagent_duration_ms: None,
            source: EventSource::Local,
            context_window: None,
            agent_type: None,
            permission_mode: None,
            cwd: None,
            transcript_path: None,
            team_name: None,
            team_agent_id: None,
            team_agent_name: None,
            team_agent_type: None,
            claude_code_version: None,
            model: None,
            session_source: None,
            stop_hook_active: None,
            agent_transcript_path: None,
            trigger: None,
            effort_level: None,
            teammate_name: None,
            task_id: None,
            task_subject: None,
            task_description: None,
            tool_response: None,
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
            notification_type: None,
            notification_title: None,
            error: None,
            is_interrupt: None,
            prompt: None,
            subagent_id: None,
            description: None,
            subagent_duration_ms: None,
            source: EventSource::Local,
            context_window: None,
            agent_type: None,
            permission_mode: None,
            cwd: None,
            transcript_path: None,
            team_name: None,
            team_agent_id: None,
            team_agent_name: None,
            team_agent_type: None,
            claude_code_version: None,
            model: None,
            session_source: None,
            stop_hook_active: None,
            agent_transcript_path: None,
            trigger: None,
            effort_level: None,
            teammate_name: None,
            task_id: None,
            task_subject: None,
            task_description: None,
            tool_response: None,
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
            notification_type: None,
            notification_title: None,
            error: None,
            is_interrupt: None,
            prompt: None,
            subagent_id: None,
            description: None,
            subagent_duration_ms: None,
            source: EventSource::Local,
            context_window: None,
            agent_type: None,
            permission_mode: None,
            cwd: None,
            transcript_path: None,
            team_name: None,
            team_agent_id: None,
            team_agent_name: None,
            team_agent_type: None,
            claude_code_version: None,
            model: None,
            session_source: None,
            stop_hook_active: None,
            agent_transcript_path: None,
            trigger: None,
            effort_level: None,
            teammate_name: None,
            task_id: None,
            task_subject: None,
            task_description: None,
            tool_response: None,
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
                notification_type: None,
                notification_title: None,
                error: None,
                is_interrupt: None,
                prompt: None,
                subagent_id: None,
                description: None,
                subagent_duration_ms: None,
                source: EventSource::Local,
                context_window: None,
                agent_type: None,
                permission_mode: None,
                cwd: None,
                transcript_path: None,
                team_name: None,
                team_agent_id: None,
                team_agent_name: None,
                team_agent_type: None,
                claude_code_version: None,
                model: None,
                session_source: None,
                stop_hook_active: None,
                agent_transcript_path: None,
                trigger: None,
                effort_level: None,
                teammate_name: None,
                task_id: None,
                task_subject: None,
                task_description: None,
                tool_response: None,
            };
            assert!(
                event.validate().is_ok(),
                "Status '{}' should be valid",
                status
            );
        }
    }

    #[test]
    fn test_claude_hook_input_teammate_idle() {
        let json = r#"{
            "session_id": "abc-123",
            "hook_event_name": "TeammateIdle",
            "team_name": "my-team",
            "teammate_name": "researcher"
        }"#;

        let input: ClaudeHookInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.hook_event_name, "TeammateIdle");
        assert_eq!(input.team_name.as_deref(), Some("my-team"));
        assert_eq!(input.teammate_name.as_deref(), Some("researcher"));
        assert!(input.task_id.is_none());
    }

    #[test]
    fn test_claude_hook_input_task_completed() {
        let json = r#"{
            "session_id": "abc-123",
            "hook_event_name": "TaskCompleted",
            "team_name": "my-team",
            "teammate_name": "coder",
            "task_id": "task-456",
            "task_subject": "Implement login",
            "task_description": "Add OAuth login flow"
        }"#;

        let input: ClaudeHookInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.hook_event_name, "TaskCompleted");
        assert_eq!(input.teammate_name.as_deref(), Some("coder"));
        assert_eq!(input.task_id.as_deref(), Some("task-456"));
        assert_eq!(input.task_subject.as_deref(), Some("Implement login"));
        assert_eq!(
            input.task_description.as_deref(),
            Some("Add OAuth login flow")
        );
    }

    #[test]
    fn test_claude_hook_input_post_tool_use_with_response() {
        let json = r#"{
            "session_id": "abc-123",
            "hook_event_name": "PostToolUse",
            "tool_name": "Bash",
            "tool_use_id": "tu-789",
            "tool_response": {"stdout": "hello", "exit_code": 0}
        }"#;

        let input: ClaudeHookInput = serde_json::from_str(json).unwrap();
        assert_eq!(input.hook_event_name, "PostToolUse");
        assert_eq!(input.tool_name.as_deref(), Some("Bash"));
        assert!(input.tool_response.is_some());
        let resp = input.tool_response.unwrap();
        assert_eq!(resp["stdout"], "hello");
        assert_eq!(resp["exit_code"], 0);
    }

    #[test]
    fn test_hook_event_deserialize_with_teammate_fields() {
        let json = r#"{
            "event": "TaskCompleted",
            "status": "working",
            "pane_id": "42",
            "project": "my-project",
            "timestamp": 1704067200,
            "teammate_name": "coder",
            "task_id": "task-1",
            "task_subject": "Fix bug",
            "task_description": "Fix the login bug"
        }"#;

        let event: HookEvent = serde_json::from_str(json).unwrap();
        assert_eq!(event.teammate_name.as_deref(), Some("coder"));
        assert_eq!(event.task_id.as_deref(), Some("task-1"));
        assert_eq!(event.task_subject.as_deref(), Some("Fix bug"));
        assert_eq!(
            event.task_description.as_deref(),
            Some("Fix the login bug")
        );
    }
}
