//! Event processing logic for Rehoboam state management
//!
//! This module contains the core `process_event()` implementation and helper functions
//! for handling Claude Code hook events.

use super::{status_to_column, Agent, AgentRole, AppState, AttentionType, Status};
use crate::config::MAX_EVENTS;
use crate::event::{EventSource, HookEvent};
use std::time::{SystemTime, UNIX_EPOCH};

/// Tools that require user input and don't fire PostToolUse until the user responds.
/// Used to detect when an agent is waiting for user input on Stop events.
const USER_INPUT_TOOLS: &[&str] = &["AskUserQuestion"];

/// v1.3: Infer agent role from subagent description keywords
///
/// Uses keyword matching to classify subagent tasks:
/// - Planner: explore, search, find, research, investigate, understand
/// - Worker: implement, fix, edit, write, create, build, update
/// - Reviewer: review, test, verify, check, validate
pub fn infer_role_from_description(description: &str) -> AgentRole {
    let desc_lower = description.to_lowercase();

    // Planner keywords (exploration/research)
    let planner_keywords = [
        "explore",
        "search",
        "find",
        "research",
        "investigate",
        "understand",
        "analyze",
        "discover",
        "locate",
        "identify",
        "scan",
        "examine",
    ];

    // Worker keywords (implementation/mutation)
    let worker_keywords = [
        "implement",
        "fix",
        "edit",
        "write",
        "create",
        "build",
        "update",
        "add",
        "modify",
        "change",
        "refactor",
        "delete",
        "remove",
    ];

    // Reviewer keywords (verification)
    let reviewer_keywords = [
        "review", "test", "verify", "check", "validate", "ensure", "confirm", "audit", "inspect",
    ];

    // Check in order of specificity
    if worker_keywords.iter().any(|kw| desc_lower.contains(kw)) {
        return AgentRole::Worker;
    }
    if reviewer_keywords.iter().any(|kw| desc_lower.contains(kw)) {
        return AgentRole::Reviewer;
    }
    if planner_keywords.iter().any(|kw| desc_lower.contains(kw)) {
        return AgentRole::Planner;
    }

    AgentRole::General
}

/// Extract file_path from tool_input JSON
///
/// Tool input for Edit/Write tools contains a "file_path" field.
/// Example: {"file_path": "/path/to/file.rs", "content": "..."}
fn extract_file_path(input: &Option<serde_json::Value>) -> Option<String> {
    input.as_ref()?.get("file_path")?.as_str().map(String::from)
}

/// Check if a tool is a Claude Code Tasks API tool
fn is_task_tool(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "TaskCreate" | "TaskUpdate" | "TaskList" | "TaskGet"
    )
}

/// Extract task subject from TaskCreate tool_input
/// Example: {"subject": "Implement user auth", "description": "..."}
fn extract_task_subject(input: &Option<serde_json::Value>) -> Option<String> {
    input.as_ref()?.get("subject")?.as_str().map(String::from)
}

/// Extract task ID from TaskUpdate/TaskGet tool_input
/// Example: {"taskId": "1", "status": "in_progress"}
fn extract_task_id(input: &Option<serde_json::Value>) -> Option<String> {
    input.as_ref()?.get("taskId")?.as_str().map(String::from)
}

/// Extract task status from TaskUpdate tool_input
fn extract_task_status(input: &Option<serde_json::Value>) -> Option<String> {
    input.as_ref()?.get("status")?.as_str().map(String::from)
}

/// Check if Task tool is running in background mode
fn is_background_task(input: &Option<serde_json::Value>) -> bool {
    input
        .as_ref()
        .and_then(|v| v.get("run_in_background"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// Extract addBlockedBy array from TaskUpdate tool_input
fn extract_blocked_by(input: &Option<serde_json::Value>) -> Vec<String> {
    input
        .as_ref()
        .and_then(|v| v.get("addBlockedBy"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Extract addBlocks array from TaskUpdate tool_input
fn extract_blocks(input: &Option<serde_json::Value>) -> Vec<String> {
    input
        .as_ref()
        .and_then(|v| v.get("addBlocks"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Extract team_name from tool_input JSON (TeamCreate, SendMessage)
fn extract_team_name_from_tool(input: &Option<serde_json::Value>) -> Option<String> {
    input.as_ref()?.get("team_name")?.as_str().map(String::from)
}

/// Extract recipient from SendMessage tool_input
fn extract_recipient(input: &Option<serde_json::Value>) -> Option<String> {
    input.as_ref()?.get("recipient")?.as_str().map(String::from)
}

/// Extract owner from TaskUpdate tool_input
fn extract_owner(input: &Option<serde_json::Value>) -> Option<String> {
    input.as_ref()?.get("owner")?.as_str().map(String::from)
}

/// Get human-readable name for column index
fn column_name(col: usize) -> &'static str {
    match col {
        0 => "attention",
        1 => "working",
        2 => "compacting",
        _ => "unknown",
    }
}

fn current_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl AppState {
    /// Process a hook event and update state
    ///
    /// Handles all v1.0 features:
    /// - Session ID tracking
    /// - Tool latency measurement (PreToolUse→PostToolUse)
    /// - Activity sparklines
    /// - Session lifecycle (start/end)
    /// - Status count caching (v1.1 optimization)
    /// - Agent limit with LRU eviction (v1.1 optimization)
    /// - Sprite agent tracking (v0.10.0)
    ///
    /// # Returns
    /// Returns `true` if the event caused a state change that requires re-render.
    #[must_use = "check if state changed to trigger re-render"]
    pub fn process_event(&mut self, event: HookEvent) -> bool {
        // Phantom agent creation: TeammateIdle/TaskCompleted carry teammate info
        // that should create/update a phantom agent entry instead of updating the leader
        if matches!(event.event.as_str(), "TeammateIdle" | "TaskCompleted") {
            if let (Some(ref teammate_name), Some(ref team_name)) =
                (&event.teammate_name, &event.team_name)
            {
                let phantom_id = format!("team:{team_name}:{teammate_name}");
                let project = event.project.clone();
                let now = current_timestamp();

                // Map session to team if we have a session_id
                if let Some(ref sid) = event.session_id {
                    self.map_session_to_team(sid.clone(), team_name.clone());
                }

                // Create or update phantom agent
                let is_new = !self.agents.contains_key(&phantom_id);
                if is_new && self.agents.len() >= crate::config::MAX_AGENTS {
                    self.evict_oldest_waiting();
                }

                let old_col = self
                    .agents
                    .get(&phantom_id)
                    .map(|a| status_to_column(&a.status));

                let agent = self
                    .agents
                    .entry(phantom_id.clone())
                    .or_insert_with(|| Agent::new(phantom_id.clone(), project.clone()));

                agent.project = project;
                agent.team_name = Some(team_name.clone());
                agent.team_agent_name = Some(teammate_name.clone());
                agent.last_event = event.event.clone();
                agent.last_update = now;

                // TeammateIdle → Attention(Waiting), TaskCompleted → Working
                let new_status = if event.event == "TeammateIdle" {
                    Status::Attention(AttentionType::Waiting)
                } else {
                    Status::Working
                };

                // Update task info if present
                if let Some(ref subject) = event.task_subject {
                    agent.current_task_subject = Some(subject.clone());
                }
                if let Some(ref task_id) = event.task_id {
                    agent.current_task_id = Some(task_id.clone());
                }

                agent.status = new_status.clone();

                // Update status counts
                let new_col = status_to_column(&new_status);
                if let Some(old) = old_col {
                    if old != new_col {
                        self.status_counts[old] = self.status_counts[old].saturating_sub(1);
                        self.status_counts[new_col] += 1;
                    }
                } else {
                    self.status_counts[new_col] += 1;
                }

                tracing::info!(
                    phantom_id = %phantom_id,
                    team = %team_name,
                    event = %event.event,
                    "Phantom agent created/updated from teammate event"
                );

                // Add to event log
                self.events.push_front(event);
                if self.events.len() > MAX_EVENTS {
                    self.events.pop_back();
                }

                return true;
            }
        }

        let pane_id = event.pane_id.clone();
        let project = event.project.clone();
        let is_new_agent = !self.agents.contains_key(&pane_id);

        // Check if this is a sprite event
        let is_sprite = matches!(event.source, EventSource::Sprite { .. });
        let sprite_id = match &event.source {
            EventSource::Sprite { sprite_id } => Some(sprite_id.clone()),
            EventSource::Local => None,
        };

        // Log event reception
        tracing::debug!(
            pane_id = %pane_id,
            event = %event.event,
            status = %event.status,
            project = %project,
            tool = ?event.tool_name,
            "Processing hook event"
        );

        // Evict oldest waiting agent if at capacity and adding new agent
        if is_new_agent && self.agents.len() >= crate::config::MAX_AGENTS {
            self.evict_oldest_waiting();
        }

        // Session-ID to team correlation: look up team BEFORE borrowing agents mutably
        let session_team = event
            .session_id
            .as_ref()
            .and_then(|sid| self.session_to_team.get(sid).cloned());

        // Track old status for count update (None if new agent)
        let old_status_col = self
            .agents
            .get(&pane_id)
            .map(|a| status_to_column(&a.status));

        // Update or create agent (sprite-aware)
        let agent = self.agents.entry(pane_id.clone()).or_insert_with(|| {
            if is_sprite {
                // Track this as a sprite agent
                self.sprite_agent_ids.insert(pane_id.clone());
                Agent::new_sprite(
                    sprite_id.clone().unwrap_or_else(|| pane_id.clone()),
                    project,
                )
            } else {
                Agent::new(pane_id.clone(), project)
            }
        });

        // Update agent state
        agent.project = event.project.clone();

        // Priority-aware status update: don't let background Working override blocking Attention
        let new_status = Status::from_str(&event.status, event.attention_type.as_deref());

        // Subagent lifecycle events should NOT change parent status
        // They report the subagent's state, not the parent's state
        let skip_status_for_subagent =
            matches!(event.event.as_str(), "SubagentStart" | "SubagentStop");

        let should_update = if skip_status_for_subagent {
            false
        } else {
            match (&agent.status, &new_status) {
                // Current status is blocking Attention (Permission or Input)
                (
                    Status::Attention(AttentionType::Permission | AttentionType::Input),
                    Status::Working,
                ) => {
                    // Allow transitions that indicate user approved/responded:
                    // - PostToolUse: Tool finished (permission was approved)
                    // - UserPromptSubmit: User sent a message
                    // Block background noise: SubagentStart, SubagentStop, PreToolUse
                    matches!(event.event.as_str(), "PostToolUse" | "UserPromptSubmit")
                }
                // Current is Attention, new is also Attention - use priority
                (Status::Attention(current_attn), Status::Attention(new_attn)) => {
                    // Only update if new attention has equal or higher priority (lower number)
                    new_attn.priority() <= current_attn.priority()
                }
                // All other cases - allow the update
                _ => true,
            }
        };

        if should_update {
            agent.status = new_status;
        }

        // v0.9.15: Refine Notification attention type using notification_type field
        if event.event == "Notification" {
            if let Some(ref ntype) = event.notification_type {
                match ntype.as_str() {
                    "permission_prompt" => {
                        agent.status = Status::Attention(AttentionType::Permission);
                    }
                    "idle_prompt" | "elicitation_dialog" => {
                        agent.status = Status::Attention(AttentionType::Input);
                    }
                    _ => {
                        // auth_success and others remain as Notification
                    }
                }
            }
        }

        // Fix: Tools requiring user input (e.g., AskUserQuestion) waiting for user response
        // should be ATTENTION, not IDLE. PostToolUse doesn't fire until user responds.
        if event.event == "Stop" {
            if let Some(tool) = &agent.current_tool {
                if USER_INPUT_TOOLS.contains(&tool.as_str()) {
                    agent.status = Status::Attention(AttentionType::Input);
                    tracing::info!(
                        pane_id = %pane_id,
                        tool = %tool,
                        "Stop with user-input tool pending → Attention(Input)"
                    );
                }
            }
        }

        agent.last_event = event.event.clone();
        agent.last_update = current_timestamp();

        // Get new status column
        let new_status_col = status_to_column(&agent.status);

        // Update status counts and log transitions
        if let Some(old_col) = old_status_col {
            if old_col != new_status_col {
                self.status_counts[old_col] = self.status_counts[old_col].saturating_sub(1);
                self.status_counts[new_status_col] += 1;
                tracing::info!(
                    pane_id = %pane_id,
                    project = %agent.project,
                    from = %column_name(old_col),
                    to = %column_name(new_status_col),
                    "Status transition"
                );
            }
        } else {
            // New agent
            self.status_counts[new_status_col] += 1;

            // Auto-select first agent if nothing is selected
            if self.selected_pane_id.is_none() {
                self.selected_pane_id = Some(pane_id.clone());
            }

            tracing::info!(
                pane_id = %pane_id,
                project = %agent.project,
                status = %column_name(new_status_col),
                "New agent registered"
            );
        }

        // Update session_id if present (v1.0)
        if let Some(sid) = &event.session_id {
            agent.session_id = Some(sid.clone());
        }

        // Claude Code 2.1.x: Update context window usage
        if let Some(ref ctx) = event.context_window {
            if let Some(pct) = ctx.used_percentage {
                agent.context_usage_percent = Some(pct);
            }
            if let Some(remaining) = ctx.remaining_percentage {
                agent.context_remaining_percent = Some(remaining);
            }
            if let Some(tokens) = ctx.total_tokens {
                agent.context_total_tokens = Some(tokens);
            }
        }

        // Claude Code 2.1.x: Update explicit agent type (overrides inferred role display)
        if let Some(ref agent_type) = event.agent_type {
            agent.explicit_agent_type = Some(agent_type.clone());
        }

        // Claude Code 2.1.x: Update permission mode
        if let Some(ref perm_mode) = event.permission_mode {
            agent.permission_mode = Some(perm_mode.clone());
        }

        // Claude Code 2.1.x: Update cwd if provided
        if let Some(ref cwd) = event.cwd {
            agent.cwd = Some(cwd.clone());
        }

        // Claude Code 2.1.x: Update transcript path
        if let Some(ref transcript) = event.transcript_path {
            agent.transcript_path = Some(transcript.clone());
        }

        // TeammateTool env vars (v3.0) - display-only monitoring
        if let Some(ref name) = event.team_name {
            agent.team_name = Some(name.clone());
        }
        if let Some(ref id) = event.team_agent_id {
            agent.team_agent_id = Some(id.clone());
        }
        if let Some(ref name) = event.team_agent_name {
            agent.team_agent_name = Some(name.clone());
        }
        if let Some(ref agent_type) = event.team_agent_type {
            agent.team_agent_type = Some(agent_type.clone());
        }

        // Session-ID to team correlation: enrich agent from pre-fetched lookup
        if agent.team_name.is_none() {
            if let Some(ref team) = session_team {
                agent.team_name = Some(team.clone());
                tracing::debug!(
                    pane_id = %pane_id,
                    team = %team,
                    "Enriched agent with team from session correlation"
                );
            }
        }

        // Claude Code version tracking
        if let Some(ref version) = event.claude_code_version {
            agent.claude_code_version = Some(version.clone());
        }

        // Claude model tracking (typically from SessionStart)
        if let Some(ref model) = event.model {
            agent.model = Some(model.clone());
        }

        // Effort level tracking
        if let Some(ref effort) = event.effort_level {
            agent.effort_level = Some(effort.clone());
        }

        // v0.9.15: Track notification details
        if event.event == "Notification" {
            agent.last_notification_type = event.notification_type.clone();
            agent.last_notification_title = event.notification_title.clone();
        }

        // v0.9.15: Track session source from SessionStart
        if event.event == "SessionStart" {
            if let Some(ref source) = event.session_source {
                agent.session_source = Some(source.clone());
            }
        }

        // v0.9.15: Track stop_hook_active from Stop/SubagentStop
        if matches!(event.event.as_str(), "Stop" | "SubagentStop") {
            agent.stop_hook_active = event.stop_hook_active.unwrap_or(false);
        }

        // v0.9.16: Track compaction from PreCompact
        if event.event == "PreCompact" {
            agent.compaction_count += 1;
            agent.last_compact_trigger = event.trigger.clone();
            tracing::info!(
                pane_id = %pane_id,
                count = agent.compaction_count,
                trigger = ?event.trigger,
                "Compaction tracked"
            );
        }

        // Track tool latency (v1.0) and role classification (v1.2)
        match event.event.as_str() {
            "PreToolUse" => {
                // Reset failed state on new tool call
                agent.last_tool_failed = false;
                agent.failed_tool_name = None;
                agent.failed_tool_error = None;
                agent.failed_tool_interrupt = false;

                if let Some(tool) = &event.tool_name {
                    agent.start_tool(tool, event.tool_use_id.as_deref(), event.timestamp);

                    // v1.2: Track tool for role inference
                    agent.record_tool(tool);

                    // v2.0: Track modified files from Edit/Write tool_input
                    if matches!(tool.as_str(), "Edit" | "Write") {
                        if let Some(file_path) = extract_file_path(&event.tool_input) {
                            agent
                                .modified_files
                                .insert(std::path::PathBuf::from(file_path));
                            tracing::debug!(
                                pane_id = %pane_id,
                                tool = %tool,
                                file = ?agent.modified_files.len(),
                                "Tracking modified file"
                            );
                        }
                    }

                    // v2.2: Track Claude Code Tasks API usage
                    if is_task_tool(tool) {
                        agent.last_task_tool = Some(tool.clone());

                        match tool.as_str() {
                            "TaskCreate" => {
                                // Extract subject from TaskCreate input
                                if let Some(subject) = extract_task_subject(&event.tool_input) {
                                    agent.current_task_subject = Some(subject.clone());

                                    // v2.1.x: Track task for dependency visualization
                                    // Note: We don't have the task ID yet (it comes in response)
                                    // We'll use a placeholder and update on PostToolUse if needed
                                    tracing::info!(
                                        pane_id = %pane_id,
                                        tool = %tool,
                                        subject = %subject,
                                        "TaskCreate detected"
                                    );
                                }
                            }
                            "TaskUpdate" => {
                                // Extract task ID and status from TaskUpdate input
                                if let Some(task_id) = extract_task_id(&event.tool_input) {
                                    let status_str = extract_task_status(&event.tool_input);
                                    let blocked_by = extract_blocked_by(&event.tool_input);
                                    let blocks = extract_blocks(&event.tool_input);

                                    // Update or create task entry
                                    let task =
                                        agent.tasks.entry(task_id.clone()).or_insert_with(|| {
                                            super::TaskInfo::new(task_id.clone(), String::new())
                                        });

                                    // Update status if provided
                                    if let Some(ref status) = status_str {
                                        task.status = super::TaskStatus::from_str(status);
                                    }

                                    // Add dependencies to this task
                                    for blocked in &blocked_by {
                                        if !task.blocked_by.contains(blocked) {
                                            task.blocked_by.push(blocked.clone());
                                        }
                                    }
                                    for blocking in &blocks {
                                        if !task.blocks.contains(blocking) {
                                            task.blocks.push(blocking.clone());
                                        }
                                    }

                                    let blocked_by_count = task.blocked_by.len();
                                    let blocks_count = task.blocks.len();

                                    // Now update the reverse relationships (separate loop to avoid borrow issues)
                                    for blocked in blocked_by {
                                        if let Some(blocker) = agent.tasks.get_mut(&blocked) {
                                            if !blocker.blocks.contains(&task_id) {
                                                blocker.blocks.push(task_id.clone());
                                            }
                                        }
                                    }
                                    for blocking in blocks {
                                        if let Some(blocked_task) = agent.tasks.get_mut(&blocking) {
                                            if !blocked_task.blocked_by.contains(&task_id) {
                                                blocked_task.blocked_by.push(task_id.clone());
                                            }
                                        }
                                    }

                                    if status_str.as_deref() == Some("in_progress") {
                                        // Worker claiming a task
                                        agent.current_task_id = Some(task_id.clone());
                                    } else if status_str.as_deref() == Some("completed") {
                                        // Task completed, clear current task
                                        if agent.current_task_id.as_deref() == Some(&task_id) {
                                            agent.current_task_id = None;
                                            agent.current_task_subject = None;
                                        }
                                    }

                                    tracing::info!(
                                        pane_id = %pane_id,
                                        tool = %tool,
                                        task_id = %task_id,
                                        status = ?status_str,
                                        blocked_by_count = blocked_by_count,
                                        blocks_count = blocks_count,
                                        "TaskUpdate detected"
                                    );
                                }
                            }
                            "TaskList" | "TaskGet" => {
                                tracing::debug!(
                                    pane_id = %pane_id,
                                    tool = %tool,
                                    "Task read operation"
                                );
                            }
                            _ => {}
                        }
                    }

                    // Team enrichment from tool_input
                    match tool.as_str() {
                        "TeamCreate" => {
                            if let Some(team_name) = extract_team_name_from_tool(&event.tool_input)
                            {
                                agent.team_name = Some(team_name);
                            }
                        }
                        "SendMessage" => {
                            if let Some(recipient) = extract_recipient(&event.tool_input) {
                                tracing::debug!(
                                    pane_id = %pane_id,
                                    recipient = %recipient,
                                    "SendMessage to teammate"
                                );
                            }
                        }
                        "TaskUpdate" => {
                            // Also extract owner for team observability
                            if let Some(owner) = extract_owner(&event.tool_input) {
                                tracing::debug!(
                                    pane_id = %pane_id,
                                    owner = %owner,
                                    "TaskUpdate with owner assignment"
                                );
                            }
                        }
                        _ => {}
                    }

                    // v2.1.x: Detect background tasks (Task tool with run_in_background: true)
                    if tool == "Task" && is_background_task(&event.tool_input) {
                        agent.has_background_tasks = true;
                        tracing::info!(
                            pane_id = %pane_id,
                            "Task with run_in_background detected"
                        );
                    }

                    // Tools requiring user input immediately need attention - transition now
                    // (PostToolUse won't fire until user responds)
                    if USER_INPUT_TOOLS.contains(&tool.as_str()) {
                        agent.status = Status::Attention(AttentionType::Input);
                        tracing::info!(
                            pane_id = %pane_id,
                            tool = %tool,
                            "PreToolUse user-input tool → Attention(Input)"
                        );
                    } else {
                        tracing::debug!(
                            pane_id = %pane_id,
                            tool = %tool,
                            "Tool started"
                        );
                    }
                }
            }
            "PostToolUse" => {
                let tool_name = agent.current_tool.clone();

                // Extract exit_code from tool_response for Bash commands
                agent.last_exit_code = None; // Reset per tool call
                if let Some(ref response) = event.tool_response {
                    if let Some(exit_code) = response.get("exit_code").and_then(|v| v.as_i64()) {
                        agent.last_exit_code = Some(exit_code);
                        if exit_code != 0 {
                            agent.failed_command_count += 1;
                            tracing::warn!(
                                pane_id = %pane_id,
                                tool = ?tool_name,
                                exit_code = exit_code,
                                "Bash command failed with non-zero exit code"
                            );
                        } else {
                            agent.successful_tool_count += 1;
                        }
                    } else {
                        agent.successful_tool_count += 1;
                    }
                } else {
                    agent.successful_tool_count += 1;
                }

                agent.end_tool(event.tool_use_id.as_deref(), event.timestamp);
                if let Some(latency) = agent.last_latency_ms {
                    tracing::info!(
                        pane_id = %pane_id,
                        tool = ?tool_name,
                        latency_ms = latency,
                        avg_ms = ?agent.avg_latency_ms,
                        "Tool completed"
                    );
                }
            }
            "PostToolUseFailure" => {
                // Track failed tool for display
                let tool_name = agent.current_tool.clone();
                agent.last_tool_failed = true;
                agent.failed_tool_name = tool_name.clone();
                agent.failed_tool_error = event.error.clone();
                agent.failed_tool_interrupt = event.is_interrupt.unwrap_or(false);
                agent.end_tool(event.tool_use_id.as_deref(), event.timestamp);
                tracing::warn!(
                    pane_id = %pane_id,
                    tool = ?tool_name,
                    error = ?event.error,
                    is_interrupt = ?event.is_interrupt,
                    "Tool failed"
                );
            }
            // v0.9.0: Subagent tracking (v1.3: enhanced with parent tracking)
            "SubagentStart" => {
                if let Some(subagent_id) = &event.subagent_id {
                    let description = event
                        .description
                        .clone()
                        .unwrap_or_else(|| "subagent".to_string());

                    // v1.3: Infer role from description keywords
                    let role = infer_role_from_description(&description);

                    // v0.9.17: Capture subagent type from event (e.g., "Bash", "Explore", "Plan")
                    let subagent_type = event.agent_type.clone();

                    agent.subagents.push(super::Subagent {
                        id: subagent_id.clone(),
                        description: description.clone(),
                        status: "running".to_string(),
                        duration_ms: None,
                        // v1.3: Parent-child tracking
                        parent_pane_id: pane_id.clone(),
                        depth: 0, // Direct child of this agent
                        role,
                        subagent_type: subagent_type.clone(),
                        transcript_path: None,
                    });
                    tracing::info!(
                        pane_id = %pane_id,
                        subagent_id = %subagent_id,
                        description = %description,
                        role = ?role,
                        subagent_type = ?subagent_type,
                        "Subagent started"
                    );
                }
            }
            "SubagentStop" => {
                if let Some(subagent_id) = &event.subagent_id {
                    // Find and update the subagent
                    if let Some(subagent) =
                        agent.subagents.iter_mut().find(|s| &s.id == subagent_id)
                    {
                        subagent.status = "completed".to_string();
                        subagent.duration_ms = event.subagent_duration_ms;
                        subagent.transcript_path = event.agent_transcript_path.clone();
                        tracing::info!(
                            pane_id = %pane_id,
                            subagent_id = %subagent_id,
                            duration_ms = ?subagent.duration_ms,
                            transcript = ?subagent.transcript_path,
                            "Subagent completed"
                        );
                    }
                }
            }
            _ => {}
        }

        // Track response state (between UserPromptSubmit and Stop)
        // This prevents timeout to IDLE while Claude is generating text
        match event.event.as_str() {
            "UserPromptSubmit" => {
                agent.in_response = true;
                tracing::debug!(pane_id = %pane_id, "Response started");
            }
            "Stop" | "SessionEnd" => {
                agent.in_response = false;
                tracing::debug!(pane_id = %pane_id, "Response ended");
            }
            _ => {}
        }

        // Set start_time on first event or session start
        if agent.start_time == 0 || event.event == "SessionStart" {
            agent.start_time = event.timestamp;

            // v2.0: Reset session-specific tracking on session start
            if event.event == "SessionStart" {
                agent.modified_files.clear();
            }
        }

        // Map session to team (deferred to avoid borrow conflicts)
        if let Some(ref sid) = event.session_id {
            if let Some(agent) = self.agents.get(&pane_id) {
                if let Some(ref team) = agent.team_name {
                    self.session_to_team.insert(sid.clone(), team.clone());
                }
            }
        }

        // Handle session end - remove agent
        if event.event == "SessionEnd" {
            // Decrement count before removal
            if let Some(agent) = self.agents.get(&pane_id) {
                let col = status_to_column(&agent.status);
                self.status_counts[col] = self.status_counts[col].saturating_sub(1);
            }
            // Clean up sprite tracking
            self.sprite_agent_ids.remove(&pane_id);
            self.agents.remove(&pane_id);
            // Clear stale selection
            if self.selected_pane_id.as_deref() == Some(&pane_id) {
                self.selected_pane_id = None;
            }
        }

        // Add to event log
        self.events.push_front(event);
        if self.events.len() > MAX_EVENTS {
            self.events.pop_back();
        }

        true // State was modified
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_team_name_from_tool() {
        let input = Some(
            serde_json::json!({"team_name": "refactor-auth", "description": "Team for auth refactor"}),
        );
        assert_eq!(
            extract_team_name_from_tool(&input),
            Some("refactor-auth".to_string())
        );

        // Missing key
        let input = Some(serde_json::json!({"description": "no team"}));
        assert_eq!(extract_team_name_from_tool(&input), None);

        // None input
        assert_eq!(extract_team_name_from_tool(&None), None);
    }

    #[test]
    fn test_extract_recipient() {
        let input = Some(
            serde_json::json!({"type": "message", "recipient": "worker-1", "content": "hello"}),
        );
        assert_eq!(extract_recipient(&input), Some("worker-1".to_string()));

        // Missing recipient
        let input = Some(serde_json::json!({"type": "broadcast", "content": "all"}));
        assert_eq!(extract_recipient(&input), None);
    }

    #[test]
    fn test_extract_owner() {
        let input = Some(serde_json::json!({"taskId": "1", "owner": "researcher"}));
        assert_eq!(extract_owner(&input), Some("researcher".to_string()));

        // Missing owner
        let input = Some(serde_json::json!({"taskId": "1", "status": "completed"}));
        assert_eq!(extract_owner(&input), None);
    }
}
