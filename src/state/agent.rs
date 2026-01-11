//! Agent state management for Claude Code sessions
//!
//! Tracks the status and activity of each Claude Code agent running in terminal panes.
//! Supports multiple terminal emulators: Tmux, WezTerm, Kitty, iTerm2.
//!
//! # Loop Mode (v0.9.0)
//! Agents can run in "Loop Mode" for Ralph-style autonomous iteration.
//! Rehoboam sends Enter keystrokes via tmux to continue loops until:
//! - Max iterations reached
//! - Stop word detected in reason
//! - Stall detected (5+ identical stop reasons)

use std::collections::VecDeque;

/// Loop mode state for Ralph-style autonomous iteration
///
/// When an agent is spawned in Loop Mode, Rehoboam monitors Stop events
/// and sends Enter keystrokes to continue the loop until a circuit breaker triggers.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum LoopMode {
    /// Not in loop mode (normal agent)
    #[default]
    None,
    /// Loop is active - will send Enter on Stop
    Active,
    /// Loop stalled (5+ identical stop reasons)
    Stalled,
    /// Loop completed (stop word found or max iterations reached)
    Complete,
}

/// A subagent spawned by the main agent (via Task tool)
///
/// Tracks subagent lifecycle from SubagentStart to SubagentStop hooks.
#[derive(Debug, Clone)]
pub struct Subagent {
    /// Subagent session ID (for correlation)
    pub id: String,
    /// Short description of what the subagent is doing
    pub description: String,
    /// Current status: "running", "completed", "failed"
    pub status: String,
    /// Start timestamp (Unix seconds)
    pub start_time: i64,
    /// Duration in milliseconds (set on SubagentStop)
    pub duration_ms: Option<u64>,
}

/// Current status of a Claude Code agent
///
/// Status determines visual representation and sorting priority:
/// - **Attention (0)**: Needs user input, highest priority (🔔)
/// - **Compacting (1)**: Context compaction in progress (🔄)
/// - **Working (2)**: Actively processing (🤖)
/// - **Idle (3)**: Waiting for input, lowest priority (⏸️)
#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    /// Session inactive, waiting for user input
    Idle,
    /// Claude is actively processing a request
    Working,
    /// Claude needs user attention (permission or input)
    Attention(AttentionType),
    /// Context compaction in progress
    Compacting,
}

impl Status {
    /// Parse status from string
    pub fn from_str(status: &str, attention_type: Option<&str>) -> Self {
        match status {
            "working" => Status::Working,
            "attention" => {
                let attn = attention_type
                    .map(AttentionType::from_str)
                    .unwrap_or(AttentionType::Input);
                Status::Attention(attn)
            }
            "compacting" => Status::Compacting,
            _ => Status::Idle,
        }
    }

    /// Priority for sorting (lower = higher priority)
    /// Kept for backwards compatibility (was used by sorted_agents)
    #[allow(dead_code)]
    pub fn priority(&self) -> u8 {
        match self {
            Status::Attention(_) => 0,
            Status::Compacting => 1,
            Status::Working => 2,
            Status::Idle => 3,
        }
    }
}

/// Type of attention the agent needs from the user
///
/// Used to determine notification style and urgency:
/// - **Permission**: Tool or action requires explicit approval
/// - **Input**: Agent is waiting for user response in the conversation
/// - **Notification**: Claude sent a notification (informational)
#[derive(Debug, Clone, PartialEq)]
pub enum AttentionType {
    /// A tool or action requires explicit user permission
    Permission,
    /// Agent is waiting for user input in the conversation
    Input,
    /// Claude sent a notification (informational alert)
    Notification,
}

impl AttentionType {
    /// Parse attention type from string
    pub fn from_str(s: &str) -> Self {
        match s {
            "permission" => AttentionType::Permission,
            "notification" => AttentionType::Notification,
            _ => AttentionType::Input,
        }
    }
}

/// A Claude Code agent instance running in a WezTerm pane
///
/// Tracks an agent's current state and activity history. Each agent is uniquely
/// identified by its `pane_id` (WezTerm pane where it's running).
///
/// # Activity Tracking
/// The `activity` field stores the last 60 activity values (0.0-1.0) for
/// sparkline visualization. Higher values indicate more activity.
///
/// # Session Timing
/// - `start_time`: When the session was first started (or restarted)
/// - `last_update`: When the last hook event was received
///
/// # Tool Latency Tracking (v1.0)
/// Measures time between PreToolUse and PostToolUse events using `tool_use_id`
/// correlation. Provides real-time insight into tool execution times.
#[derive(Debug, Clone)]
pub struct Agent {
    /// WezTerm pane ID (unique identifier for this agent)
    pub pane_id: String,
    /// Git project name or current directory name
    pub project: String,
    /// Current agent status
    pub status: Status,
    /// Session start time (Unix timestamp in seconds)
    pub start_time: i64,
    /// Last event timestamp (Unix timestamp in seconds)
    pub last_update: i64,
    /// Name of the last hook event received
    pub last_event: String,
    /// Activity history for sparkline visualization (0.0-1.0)
    pub activity: VecDeque<f64>,

    // v1.0 rich data fields
    /// Claude Code session identifier
    pub session_id: Option<String>,
    /// Currently executing tool (Bash, Read, Write, etc.)
    pub current_tool: Option<String>,
    /// PreToolUse timestamp for latency calculation
    pub pending_tool_start: Option<i64>,
    /// tool_use_id for correlating Pre→Post events
    pub pending_tool_use_id: Option<String>,
    /// Last tool execution time in milliseconds
    pub last_latency_ms: Option<u64>,
    /// Running average latency in milliseconds
    pub avg_latency_ms: Option<u64>,
    /// Total tool calls this session
    pub total_tool_calls: u32,
    /// True when Claude is actively responding (between UserPromptSubmit and Stop)
    /// Prevents timeout to IDLE while Claude is generating text (no tool hooks)
    pub in_response: bool,

    // v0.9.0 Loop Mode fields
    /// Current loop mode state
    pub loop_mode: LoopMode,
    /// Current iteration count (incremented on each Stop event)
    pub loop_iteration: u32,
    /// Maximum iterations before stopping (circuit breaker)
    pub loop_max: u32,
    /// Stop word to detect completion (e.g., "DONE")
    pub loop_stop_word: String,
    /// Last 5 stop reasons for stall detection
    pub loop_last_reasons: VecDeque<String>,

    // v0.9.0 Subagent tracking
    /// Subagents spawned by this agent
    pub subagents: Vec<Subagent>,
}

impl Agent {
    pub fn new(pane_id: String, project: String) -> Self {
        Self {
            pane_id,
            project,
            status: Status::Idle,
            start_time: 0,
            last_update: 0,
            last_event: String::new(),
            activity: VecDeque::with_capacity(60),
            // v1.0 fields
            session_id: None,
            current_tool: None,
            pending_tool_start: None,
            pending_tool_use_id: None,
            last_latency_ms: None,
            avg_latency_ms: None,
            total_tool_calls: 0,
            in_response: false,
            // v0.9.0 Loop Mode fields
            loop_mode: LoopMode::None,
            loop_iteration: 0,
            loop_max: 50, // Default max iterations
            loop_stop_word: "DONE".to_string(),
            loop_last_reasons: VecDeque::with_capacity(5),
            // v0.9.0 Subagent tracking
            subagents: Vec::new(),
        }
    }

    /// Calculate elapsed time since start
    pub fn elapsed_secs(&self) -> i64 {
        if self.start_time == 0 {
            return 0;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        now - self.start_time
    }

    /// Format elapsed time for display
    pub fn elapsed_display(&self) -> String {
        let secs = self.elapsed_secs();
        if secs == 0 {
            return "--".to_string();
        }
        if secs < 60 {
            format!("{}s", secs)
        } else if secs < 3600 {
            format!("{}m {:02}s", secs / 60, secs % 60)
        } else {
            format!("{}h {:02}m", secs / 3600, (secs % 3600) / 60)
        }
    }

    /// Record PreToolUse event for latency tracking
    ///
    /// Called when a tool starts executing. Stores the tool name, timestamp,
    /// and tool_use_id for later correlation with PostToolUse.
    pub fn start_tool(&mut self, tool: &str, tool_use_id: Option<&str>, timestamp: i64) {
        self.current_tool = Some(tool.to_string());
        self.pending_tool_start = Some(timestamp);
        self.pending_tool_use_id = tool_use_id.map(String::from);
    }

    /// Record PostToolUse event and calculate latency
    ///
    /// Called when a tool finishes executing. If tool_use_id matches the pending
    /// tool, calculates the latency and updates statistics.
    ///
    /// # Latency Calculation
    /// - `last_latency_ms`: Time for this specific tool call
    /// - `avg_latency_ms`: Running average across all tool calls this session
    pub fn end_tool(&mut self, tool_use_id: Option<&str>, timestamp: i64) {
        // Verify tool_use_id matches (if both are present)
        if let (Some(pending), Some(incoming)) = (&self.pending_tool_use_id, tool_use_id) {
            if pending != incoming {
                // IDs don't match, this PostToolUse is for a different tool
                return;
            }
        }

        // Calculate latency if we have a start time
        if let Some(start) = self.pending_tool_start {
            let latency = ((timestamp - start) * 1000) as u64;
            self.last_latency_ms = Some(latency);

            // Update running average
            self.total_tool_calls += 1;
            let avg = self.avg_latency_ms.unwrap_or(0);
            self.avg_latency_ms = Some(
                (avg * (self.total_tool_calls - 1) as u64 + latency) / self.total_tool_calls as u64,
            );
        }

        // Clear pending tool state
        self.current_tool = None;
        self.pending_tool_start = None;
        self.pending_tool_use_id = None;
    }

    /// Get display string for tool/latency column
    ///
    /// Shows current tool if executing, otherwise last latency.
    pub fn tool_display(&self) -> String {
        if let Some(tool) = &self.current_tool {
            truncate_tool_name(tool)
        } else if let Some(ms) = self.last_latency_ms {
            format_latency(ms)
        } else {
            "-".to_string()
        }
    }
}

/// Truncate tool name for display (max 12 chars)
fn truncate_tool_name(tool: &str) -> String {
    if tool.len() <= 12 {
        tool.to_string()
    } else {
        format!("{}…", &tool[..11])
    }
}

/// Format latency for display
fn format_latency(ms: u64) -> String {
    if ms < 1000 {
        format!("{}ms", ms)
    } else {
        format!("{:.1}s", ms as f64 / 1000.0)
    }
}
