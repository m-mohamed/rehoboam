//! Event processing logic for Rehoboam state management
//!
//! This module contains the core `process_event()` implementation and helper functions
//! for handling Claude Code hook events.

use super::{status_to_column, Agent, AgentRole, AppState, AttentionType, Status};
use crate::config::{MAX_EVENTS, MAX_SPARKLINE_POINTS};
use crate::event::{EventSource, HookEvent};
use std::time::{SystemTime, UNIX_EPOCH};

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

        // Fix: AskUserQuestion waiting for user response should be ATTENTION, not IDLE
        // PostToolUse doesn't fire until user responds, so current_tool is still set
        if event.event == "Stop" {
            if let Some(tool) = &agent.current_tool {
                if tool == "AskUserQuestion" {
                    agent.status = Status::Attention(AttentionType::Input);
                    tracing::info!(
                        pane_id = %pane_id,
                        tool = %tool,
                        "Stop with AskUserQuestion pending → Attention(Input)"
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

        // Claude Code version tracking
        if let Some(ref version) = event.claude_code_version {
            agent.claude_code_version = Some(version.clone());
        }

        // Claude model tracking (typically from SessionStart)
        if let Some(ref model) = event.model {
            agent.model = Some(model.clone());
        }

        // Track tool latency (v1.0) and role classification (v1.2)
        match event.event.as_str() {
            "PreToolUse" => {
                // Reset failed state on new tool call
                agent.last_tool_failed = false;
                agent.failed_tool_name = None;

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
                                    let task = agent
                                        .tasks
                                        .entry(task_id.clone())
                                        .or_insert_with(|| {
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

                    // v2.1.x: Detect background tasks (Task tool with run_in_background: true)
                    if tool == "Task" && is_background_task(&event.tool_input) {
                        agent.has_background_tasks = true;
                        tracing::info!(
                            pane_id = %pane_id,
                            "Task with run_in_background detected"
                        );
                    }

                    // AskUserQuestion immediately needs user input - transition now
                    // (PostToolUse won't fire until user responds)
                    if tool == "AskUserQuestion" {
                        agent.status = Status::Attention(AttentionType::Input);
                        tracing::info!(
                            pane_id = %pane_id,
                            "PreToolUse AskUserQuestion → Attention(Input)"
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
                agent.end_tool(event.tool_use_id.as_deref(), event.timestamp);
                tracing::warn!(
                    pane_id = %pane_id,
                    tool = ?tool_name,
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

                    agent.subagents.push(super::Subagent {
                        id: subagent_id.clone(),
                        description: description.clone(),
                        status: "running".to_string(),
                        duration_ms: None,
                        // v1.3: Parent-child tracking
                        parent_pane_id: pane_id.clone(),
                        depth: 0, // Direct child of this agent
                        role,
                    });
                    tracing::info!(
                        pane_id = %pane_id,
                        subagent_id = %subagent_id,
                        description = %description,
                        role = ?role,
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
                        tracing::info!(
                            pane_id = %pane_id,
                            subagent_id = %subagent_id,
                            duration_ms = ?subagent.duration_ms,
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

                // Capture session start commit for session-scoped diffs
                if let Some(ref working_dir) = agent.working_dir {
                    let git = crate::git::GitController::new(working_dir.clone());
                    agent.session_start_commit = git.head_commit().ok();
                    tracing::debug!(
                        pane_id = %pane_id,
                        commit = ?agent.session_start_commit,
                        "Captured session start commit"
                    );
                }
            }
        }

        // Add activity point for sparkline
        let activity_value = match &agent.status {
            Status::Working => 1.0,
            Status::Attention(attn) => match attn {
                AttentionType::Permission | AttentionType::Input => 0.8,
                AttentionType::Notification => 0.5,
                AttentionType::Waiting => 0.1,
            },
            Status::Compacting => 0.6,
        };
        agent.activity.push_back(activity_value);
        if agent.activity.len() > MAX_SPARKLINE_POINTS {
            agent.activity.pop_front();
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
        }

        // Add to event log
        self.events.push_front(event);
        if self.events.len() > MAX_EVENTS {
            self.events.pop_back();
        }

        true // State was modified
    }
}
