//! Status derivation from Claude Code hook events
//!
//! This module provides a single source of truth for mapping hook event names
//! to Rehoboam status values.

/// Derive rehoboam status from a hook event name.
///
/// Maps Claude Code hook events to Rehoboam status:
/// - working: Claude is actively processing
/// - attention: Claude needs user attention
/// - compacting: Context maintenance in progress
///
/// Attention types:
/// - permission: Tool needs explicit approval (blocking)
/// - notification: Informational alert
/// - waiting: Ready for new prompt (was "idle")
///
/// # Returns
/// Tuple of (status, optional attention_type) as static string references
pub fn derive_status_from_event(hook_name: &str) -> (&'static str, Option<&'static str>) {
    match hook_name {
        // WORKING: Claude is actively doing something
        "UserPromptSubmit" => ("working", None), // User sent message, Claude processing
        "PreToolUse" => ("working", None),       // About to run a tool
        "PostToolUse" => ("working", None),      // Just finished a tool, may continue
        "SubagentStart" => ("working", None),    // Spawned a subagent
        "SubagentStop" => ("working", None),     // Subagent finished, may continue
        "Setup" => ("working", None),            // Claude Code 2.1.x: initialization/setup phase
        "PostToolUseFailure" => ("working", None), // Tool failed, but Claude continues

        // ATTENTION: Claude is BLOCKED waiting for explicit user approval
        "PermissionRequest" => ("attention", Some("permission")),

        // ATTENTION(Waiting): Claude is waiting for user input (was "idle")
        "SessionStart" => ("attention", Some("waiting")), // Session started, waiting for prompt
        "Stop" => ("attention", Some("waiting")),         // Claude finished responding
        "SessionEnd" => ("attention", Some("waiting")),   // Session ended

        // ATTENTION(Notification): Informational alert
        "Notification" => ("attention", Some("notification")),

        // COMPACTING: Context maintenance
        "PreCompact" => ("compacting", None),
        "PostCompact" => ("compacting", None),

        // Unknown hooks default to attention(waiting) (conservative)
        _ => ("attention", Some("waiting")),
    }
}

/// Derive rehoboam status with owned strings (for remote event processing).
///
/// This is a convenience wrapper around `derive_status_from_event` that returns
/// owned Strings instead of static references. Useful for sprite event processing
/// where owned values are needed.
///
/// # Returns
/// Tuple of (status, optional attention_type) as owned Strings
pub fn derive_status_owned(hook_name: &str) -> (String, Option<String>) {
    let (status, attention) = derive_status_from_event(hook_name);
    (status.to_string(), attention.map(|s| s.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_working_events() {
        let working_events = [
            "UserPromptSubmit",
            "PreToolUse",
            "PostToolUse",
            "PostToolUseFailure",
            "SubagentStart",
            "SubagentStop",
            "Setup",
        ];
        for event in working_events {
            let (status, attention) = derive_status_from_event(event);
            assert_eq!(status, "working", "event: {}", event);
            assert!(attention.is_none(), "event: {}", event);
        }
    }

    #[test]
    fn test_attention_events() {
        // Permission request
        let (status, attention) = derive_status_from_event("PermissionRequest");
        assert_eq!(status, "attention");
        assert_eq!(attention, Some("permission"));

        // Notification
        let (status, attention) = derive_status_from_event("Notification");
        assert_eq!(status, "attention");
        assert_eq!(attention, Some("notification"));

        // Waiting events
        for event in ["SessionStart", "Stop", "SessionEnd"] {
            let (status, attention) = derive_status_from_event(event);
            assert_eq!(status, "attention", "event: {}", event);
            assert_eq!(attention, Some("waiting"), "event: {}", event);
        }
    }

    #[test]
    fn test_compacting_events() {
        for event in ["PreCompact", "PostCompact"] {
            let (status, attention) = derive_status_from_event(event);
            assert_eq!(status, "compacting", "event: {}", event);
            assert!(attention.is_none(), "event: {}", event);
        }
    }

    #[test]
    fn test_unknown_event_defaults_to_waiting() {
        let (status, attention) = derive_status_from_event("UnknownEvent");
        assert_eq!(status, "attention");
        assert_eq!(attention, Some("waiting"));
    }

    #[test]
    fn test_owned_version() {
        let (status, attention) = derive_status_owned("PermissionRequest");
        assert_eq!(status, "attention");
        assert_eq!(attention, Some("permission".to_string()));
    }
}
