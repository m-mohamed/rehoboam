//! Agent state management for Claude Code sessions
//!
//! Tracks the status and activity of each Claude Code agent running in terminal panes.
//! Supports multiple terminal emulators: Tmux, WezTerm, Kitty, iTerm2.

use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;

/// Agent role classification based on tool usage patterns
///
/// Inspired by Cursor's hierarchical agent model (Planner/Worker/Judge).
/// Role is inferred from recent tool calls - not explicitly set.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum AgentRole {
    /// Exploring/planning - frequent Read, Glob, Grep, no edits
    /// Identified by >80% read-only tools in recent calls
    Planner,
    /// Executing tasks - Edit, Write, Bash
    /// Identified by any mutation tool calls
    Worker,
    /// Reviewing/evaluating - Read after Write, test runs
    /// Identified by Read tools following recent Edit/Write
    Reviewer,
    /// Unknown or mixed behavior (default)
    #[default]
    General,
}

/// Task status for Claude Code Tasks API
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum TaskStatus {
    /// Task is waiting to be started
    #[default]
    Pending,
    /// Task is currently being worked on
    InProgress,
    /// Task has been completed
    Completed,
}

impl TaskStatus {
    /// Parse status from string
    pub fn from_str(s: &str) -> Self {
        match s {
            "in_progress" => TaskStatus::InProgress,
            "completed" => TaskStatus::Completed,
            _ => TaskStatus::Pending,
        }
    }

    /// Get display indicator for status
    pub fn indicator(&self) -> &'static str {
        match self {
            TaskStatus::Pending => "○",
            TaskStatus::InProgress => "●",
            TaskStatus::Completed => "✓",
        }
    }
}

/// Information about a Claude Code Task (from Tasks API)
///
/// Tracks task metadata including dependencies for visualization.
#[derive(Debug, Clone)]
pub struct TaskInfo {
    /// Task ID (from TaskCreate response or TaskUpdate input)
    pub id: String,
    /// Task subject/title
    pub subject: String,
    /// Current task status
    pub status: TaskStatus,
    /// Task IDs that block this task (must complete first)
    pub blocked_by: Vec<String>,
    /// Task IDs that this task blocks (waiting on this one)
    pub blocks: Vec<String>,
}

impl TaskInfo {
    pub fn new(id: String, subject: String) -> Self {
        Self {
            id,
            subject,
            status: TaskStatus::Pending,
            blocked_by: Vec::new(),
            blocks: Vec::new(),
        }
    }
}

/// A subagent spawned by the main agent (via Task tool)
///
/// Tracks subagent lifecycle from SubagentStart to SubagentStop hooks.
/// v1.3: Extended with parent tracking for hierarchical visualization.
#[derive(Debug, Clone)]
pub struct Subagent {
    /// Subagent session ID (for correlation)
    pub id: String,
    /// Short description of what the subagent is doing
    #[allow(dead_code)] // Tracked for future subagent tree display
    pub description: String,
    /// Current status: "running", "completed", "failed"
    pub status: String,
    /// Duration in milliseconds (set on SubagentStop)
    pub duration_ms: Option<u64>,

    // v1.3: Parent-child relationship tracking
    /// Parent pane ID (the agent that spawned this subagent).
    /// Used for future hierarchical subagent tree visualization.
    #[allow(dead_code)] // Tracked for future nested tree display
    pub parent_pane_id: String,
    /// Nesting depth (0 = root agent's direct child, 1 = grandchild, etc.).
    /// Used for hierarchical subagent tree visualization.
    #[allow(dead_code)] // Tracked for future nested tree display
    pub depth: u8,
    /// Inferred role based on subagent description
    #[allow(dead_code)] // Tracked for future subagent tree display
    pub role: AgentRole,

    /// Subagent type from SubagentStart/Stop agent_type field
    /// e.g., "Bash", "Explore", "Plan", "general-purpose"
    pub subagent_type: Option<String>,
}

/// Current status of a Claude Code agent
///
/// Status determines visual representation and column placement:
/// - **Attention (0)**: Needs user attention - includes Permission, Input, Notification, Waiting
/// - **Compacting (1)**: Context compaction in progress (🔄)
/// - **Working (2)**: Actively processing (🤖)
///
/// Note: Idle state has been merged into Attention(Waiting) for better visibility
#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    /// Claude needs user attention (permission, input, notification, or waiting)
    Attention(AttentionType),
    /// Claude is actively processing a request
    Working,
    /// Context compaction in progress
    Compacting,
}

impl Status {
    /// Parse status from string
    pub fn from_str(status: &str, attention_type: Option<&str>) -> Self {
        match status {
            "working" => Status::Working,
            "attention" => {
                let attn = attention_type.map_or(AttentionType::Input, AttentionType::from_str);
                Status::Attention(attn)
            }
            "compacting" => Status::Compacting,
            // Previously "idle" - now maps to Attention(Waiting)
            _ => Status::Attention(AttentionType::Waiting),
        }
    }

    /// Priority for sorting (lower = higher priority)
    pub fn priority(&self) -> u8 {
        match self {
            Status::Attention(_) => 0,
            Status::Compacting => 1,
            Status::Working => 2,
        }
    }
}

/// Type of attention the agent needs from the user
///
/// Used to determine notification style and urgency:
/// - **Permission**: Tool or action requires explicit approval (highest priority)
/// - **Input**: Agent is waiting for user response in the conversation
/// - **Notification**: Claude sent a notification (informational)
/// - **Waiting**: Agent is idle, ready for new prompt (lowest priority, was Status::Idle)
#[derive(Debug, Clone, PartialEq)]
pub enum AttentionType {
    /// A tool or action requires explicit user permission
    Permission,
    /// Agent is waiting for user input in the conversation
    Input,
    /// Claude sent a notification (informational alert)
    Notification,
    /// Agent is idle, waiting for user to start interaction (was Status::Idle)
    Waiting,
}

impl AttentionType {
    /// Parse attention type from string
    pub fn from_str(s: &str) -> Self {
        match s {
            "permission" => AttentionType::Permission,
            "notification" => AttentionType::Notification,
            "waiting" => AttentionType::Waiting,
            _ => AttentionType::Input,
        }
    }

    /// Priority for sorting within Attention column (lower = higher priority)
    pub fn priority(&self) -> u8 {
        match self {
            AttentionType::Permission => 0,
            AttentionType::Input => 1,
            AttentionType::Notification => 2,
            AttentionType::Waiting => 3,
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
    /// Prevents timeout to Waiting while Claude is generating text (no tool hooks)
    pub in_response: bool,

    // v0.9.0 Subagent tracking
    /// Subagents spawned by this agent
    pub subagents: Vec<Subagent>,

    // v0.10.0 Sprite tracking
    /// True if this agent is running in a remote Sprite VM
    pub is_sprite: bool,
    /// Sprite ID (same as pane_id for sprite agents)
    pub sprite_id: Option<String>,

    // v1.0 Git operations
    /// Working directory for git operations (worktree path)
    pub working_dir: Option<std::path::PathBuf>,

    // v1.2 Agent role classification (Cursor-inspired)
    /// Inferred agent role based on tool usage patterns
    pub role: AgentRole,
    /// Recent tool names for role inference (last 10 tools)
    pub tool_history: VecDeque<String>,

    // v2.0 Per-agent file tracking (Phase 7)
    /// Files modified by this agent (tracked from Edit/Write tool_input)
    pub modified_files: HashSet<PathBuf>,
    /// Git commit hash at session start (for session-scoped diffs)
    pub session_start_commit: Option<String>,

    // Claude Code 2.1.x integration fields
    /// Context window usage percentage (0.0-100.0)
    pub context_usage_percent: Option<f64>,
    /// Context window remaining percentage (0.0-100.0) - Claude Code 2.1.6+
    pub context_remaining_percent: Option<f64>,
    /// Total tokens in context
    pub context_total_tokens: Option<u64>,
    /// Agent type from --agent flag (explicit, overrides inferred role)
    pub explicit_agent_type: Option<String>,
    /// Permission mode (plan, acceptEdits, etc.)
    pub permission_mode: Option<String>,
    /// Working directory (from cwd field, may differ from project)
    pub cwd: Option<String>,
    /// Transcript path for linking to conversation
    pub transcript_path: Option<String>,

    // Claude Code Tasks API integration (v2.2)
    /// Last task tool used (TaskCreate, TaskUpdate, TaskList, TaskGet)
    pub last_task_tool: Option<String>,
    /// Current task subject being worked on
    pub current_task_subject: Option<String>,
    /// Current task ID if claimed (for Workers)
    pub current_task_id: Option<String>,
    /// Task list ID for multi-agent coordination (CLAUDE_CODE_TASK_LIST_ID)
    pub task_list_id: Option<String>,

    // TeammateTool integration (v3.0) - display-only fields
    /// Team name from CLAUDE_CODE_TEAM_NAME env var
    pub team_name: Option<String>,
    /// Agent ID within team from CLAUDE_CODE_AGENT_ID env var
    pub team_agent_id: Option<String>,
    /// Agent name within team from CLAUDE_CODE_AGENT_NAME env var
    pub team_agent_name: Option<String>,
    /// Agent type from CLAUDE_CODE_AGENT_TYPE env var (e.g., "planner", "worker")
    pub team_agent_type: Option<String>,

    // Claude Code version tracking
    /// Claude Code version from CLAUDE_CODE_VERSION env var
    pub claude_code_version: Option<String>,

    /// Claude model name (e.g., "opus-4.5", "sonnet-4")
    pub model: Option<String>,

    // Background task tracking (v2.1.x)
    /// True if this agent has spawned background tasks (Task tool with run_in_background: true)
    pub has_background_tasks: bool,

    // Task dependency tracking (v2.1.x)
    /// Tasks tracked by this agent (keyed by task ID)
    pub tasks: std::collections::HashMap<String, TaskInfo>,

    // Tool failure tracking (v2.1.x)
    /// True if the last tool call failed (PostToolUseFailure)
    pub last_tool_failed: bool,
    /// Name of the failed tool (for display)
    pub failed_tool_name: Option<String>,
    /// Error message from last tool failure
    pub failed_tool_error: Option<String>,
    /// Whether the last failure was a user interrupt
    pub failed_tool_interrupt: bool,

    // Notification tracking (v0.9.15)
    /// Last notification type (permission_prompt, idle_prompt, auth_success, elicitation_dialog)
    pub last_notification_type: Option<String>,
    /// Last notification title
    pub last_notification_title: Option<String>,

    // Session metadata (v0.9.15)
    /// Session source from SessionStart (startup, resume, clear, compact)
    pub session_source: Option<String>,
    /// Whether stop hook is active (Claude continues after Stop)
    pub stop_hook_active: bool,

    /// Effort level from CLAUDE_CODE_EFFORT_LEVEL env var
    pub effort_level: Option<String>,
}

impl Agent {
    pub fn new(pane_id: String, project: String) -> Self {
        Self {
            pane_id,
            project,
            status: Status::Attention(AttentionType::Waiting),
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
            // v0.9.0 Subagent tracking
            subagents: Vec::new(),
            // v0.10.0 Sprite tracking
            is_sprite: false,
            sprite_id: None,
            // v1.0 Git operations
            working_dir: None,
            // v1.2 Agent role classification
            role: AgentRole::General,
            tool_history: VecDeque::with_capacity(10),
            // v2.0 Per-agent file tracking
            modified_files: HashSet::new(),
            session_start_commit: None,
            // Claude Code 2.1.x integration fields
            context_usage_percent: None,
            context_remaining_percent: None,
            context_total_tokens: None,
            explicit_agent_type: None,
            permission_mode: None,
            cwd: None,
            transcript_path: None,
            // Claude Code Tasks API integration (v2.2)
            last_task_tool: None,
            current_task_subject: None,
            current_task_id: None,
            task_list_id: None,
            // TeammateTool integration (v3.0)
            team_name: None,
            team_agent_id: None,
            team_agent_name: None,
            team_agent_type: None,
            // Claude Code version
            claude_code_version: None,
            // Claude model
            model: None,
            // Background task tracking
            has_background_tasks: false,
            // Task dependency tracking
            tasks: std::collections::HashMap::new(),
            // Tool failure tracking
            last_tool_failed: false,
            failed_tool_name: None,
            failed_tool_error: None,
            failed_tool_interrupt: false,
            // Notification tracking (v0.9.15)
            last_notification_type: None,
            last_notification_title: None,
            // Session metadata (v0.9.15)
            session_source: None,
            stop_hook_active: false,
            // Effort level
            effort_level: None,
        }
    }

    /// Create a new sprite agent (running in remote VM)
    pub fn new_sprite(sprite_id: String, project: String) -> Self {
        let mut agent = Self::new(sprite_id.clone(), project);
        agent.is_sprite = true;
        agent.sprite_id = Some(sprite_id);
        agent
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
            format!("{secs}s")
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
                // IDs don't match - log warning and still clean up pending state
                tracing::warn!(
                    pane_id = %self.pane_id,
                    pending_id = %pending,
                    incoming_id = %incoming,
                    current_tool = ?self.current_tool,
                    "Tool use ID mismatch - skipping latency, cleaning up pending state"
                );
                // Clean up to avoid stale tool state
                self.current_tool = None;
                self.pending_tool_start = None;
                self.pending_tool_use_id = None;
                return;
            }
        }
        // Log if we have no pending ID but got one, or vice versa (helpful for debugging)
        if self.pending_tool_use_id.is_none() && tool_use_id.is_some() {
            tracing::debug!(
                pane_id = %self.pane_id,
                incoming_id = ?tool_use_id,
                "PostToolUse has ID but no pending PreToolUse ID"
            );
        }

        // Calculate latency if we have a start time
        if let Some(start) = self.pending_tool_start {
            let latency = ((timestamp - start) * 1000) as u64;
            self.last_latency_ms = Some(latency);

            // Update running average
            self.total_tool_calls += 1;
            let avg = self.avg_latency_ms.unwrap_or(0);
            self.avg_latency_ms = Some(
                (avg * u64::from(self.total_tool_calls - 1) + latency)
                    / u64::from(self.total_tool_calls),
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

    // =========================================================================
    // v1.2 Role Classification (Cursor-inspired Planner/Worker/Reviewer)
    // =========================================================================

    /// Read-only tools (used for Planner role detection)
    const READ_ONLY_TOOLS: &'static [&'static str] = &[
        "Read",
        "Glob",
        "Grep",
        "WebFetch",
        "WebSearch",
        "ListMcpResourcesTool",
        "ReadMcpResourceTool",
        "Task",
        "TodoRead",
        // Claude Code Tasks API (read operations)
        "TaskList",
        "TaskGet",
        // User interaction / mode transition tools (read-only side-effects)
        "Skill",
        "AskUserQuestion",
        "EnterPlanMode",
        "ExitPlanMode",
    ];

    /// Mutation tools (used for Worker role detection)
    const MUTATION_TOOLS: &'static [&'static str] = &[
        "Edit",
        "Write",
        "Bash",
        "NotebookEdit",
        "TodoWrite",
        // Claude Code Tasks API (write operations)
        "TaskCreate",
        "TaskUpdate",
        // Team coordination tools (write/side-effect operations)
        "SendMessage",
        "TeamCreate",
        "TeamDelete",
    ];

    /// Record a tool call and update role inference
    ///
    /// Called on each PreToolUse event. Maintains a rolling history of the
    /// last 10 tools and infers agent role from the pattern.
    pub fn record_tool(&mut self, tool: &str) {
        // Add to history (max 10)
        if self.tool_history.len() >= 10 {
            self.tool_history.pop_front();
        }
        self.tool_history.push_back(tool.to_string());

        // Re-infer role
        self.role = self.infer_role();
    }

    /// Infer agent role from tool history
    ///
    /// Detection heuristics (from Cursor research):
    /// - Planner: >80% read-only tools, no mutations
    /// - Worker: Any mutation tool calls
    /// - Reviewer: Read tools after recent Edit/Write
    /// - General: Default/mixed behavior
    fn infer_role(&self) -> AgentRole {
        if self.tool_history.is_empty() {
            return AgentRole::General;
        }

        let total = self.tool_history.len();
        let read_only_count = self
            .tool_history
            .iter()
            .filter(|t| Self::READ_ONLY_TOOLS.contains(&t.as_str()))
            .count();
        let mutation_count = self
            .tool_history
            .iter()
            .filter(|t| Self::MUTATION_TOOLS.contains(&t.as_str()))
            .count();

        // Check for Reviewer: Recent mutation followed by reads
        if let Some(last_mutation_idx) = self
            .tool_history
            .iter()
            .rposition(|t| Self::MUTATION_TOOLS.contains(&t.as_str()))
        {
            // If mutation was in last 5 tools and followed by reads
            if last_mutation_idx < total.saturating_sub(1) {
                let reads_after_mutation = self
                    .tool_history
                    .iter()
                    .skip(last_mutation_idx + 1)
                    .filter(|t| Self::READ_ONLY_TOOLS.contains(&t.as_str()))
                    .count();
                if reads_after_mutation >= 2 {
                    return AgentRole::Reviewer;
                }
            }
        }

        // Worker: Any mutations present
        if mutation_count > 0 {
            return AgentRole::Worker;
        }

        // Planner: >80% read-only (and at least 3 tools for confidence)
        if total >= 3 && (read_only_count as f64 / total as f64) >= 0.8 {
            return AgentRole::Planner;
        }

        AgentRole::General
    }

    /// Get task progress as (completed, total)
    ///
    /// Returns (0, 0) if no tasks are tracked.
    pub fn task_progress(&self) -> (usize, usize) {
        let total = self.tasks.len();
        let completed = self
            .tasks
            .values()
            .filter(|t| matches!(t.status, TaskStatus::Completed))
            .count();
        (completed, total)
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
        format!("{ms}ms")
    } else {
        format!("{:.1}s", ms as f64 / 1000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_role_classification_general_default() {
        let agent = Agent::new("%0".to_string(), "test".to_string());
        assert_eq!(agent.role, AgentRole::General);
    }

    #[test]
    fn test_role_classification() {
        // Table-driven test for role classification based on tool usage patterns
        let cases: Vec<(Vec<&str>, AgentRole, &str)> = vec![
            // (tools, expected_role, description)
            (
                vec!["Read", "Glob", "Grep", "Read", "WebSearch"],
                AgentRole::Planner,
                "5 read-only tools -> Planner (>80% read-only)",
            ),
            (
                vec!["Read", "Edit"],
                AgentRole::Worker,
                "Any mutation tool -> Worker",
            ),
            (
                vec!["Edit", "Read", "Read", "Grep"],
                AgentRole::Reviewer,
                "Edit followed by 2+ reads -> Reviewer",
            ),
        ];

        for (tools, expected_role, desc) in cases {
            let mut agent = Agent::new("%0".to_string(), "test".to_string());
            for tool in tools {
                agent.record_tool(tool);
            }
            assert_eq!(agent.role, expected_role, "{}", desc);
        }
    }

    #[test]
    fn test_role_history_rolling_window() {
        let mut agent = Agent::new("%0".to_string(), "test".to_string());

        // Fill up with 10 reads (Planner)
        for _ in 0..10 {
            agent.record_tool("Read");
        }
        assert_eq!(agent.role, AgentRole::Planner);

        // Add an Edit - should become Worker (still in history)
        agent.record_tool("Edit");
        assert_eq!(agent.role, AgentRole::Worker);

        // Add 10 more reads - Edit should roll out, become Planner again
        for _ in 0..10 {
            agent.record_tool("Read");
        }
        assert_eq!(agent.role, AgentRole::Planner);
    }
}
