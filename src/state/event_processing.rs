//! Event processing logic for Rehoboam state management
//!
//! This module contains the core `process_event()` implementation and helper functions
//! for handling Claude Code hook events.

use super::{status_to_column, Agent, AgentRole, AppState, AttentionType, LoopMode, Status};
use crate::config::{MAX_EVENTS, MAX_SPARKLINE_POINTS};
use crate::event::{EventSource, HookEvent};
use crate::rehoboam_loop;
use crate::state::loop_handling::spawn_fresh_rehoboam_session;
use crate::tmux::TmuxController;
use crate::notify;
use once_cell::sync::Lazy;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// Guard against concurrent respawns for the same pane
static RESPAWN_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

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

            // Check for pending loop config from spawn
            if let Some(loop_config) = self.pending_loop_configs.remove(&pane_id) {
                agent.loop_mode = LoopMode::Active;
                agent.loop_max = loop_config.max_iterations;
                agent.loop_stop_word = loop_config.stop_word.clone();
                agent.loop_dir = loop_config.loop_dir.clone();
                agent.working_dir = loop_config.working_dir.clone();
                tracing::info!(
                    pane_id = %pane_id,
                    max = loop_config.max_iterations,
                    stop_word = %loop_config.stop_word,
                    loop_dir = ?loop_config.loop_dir,
                    working_dir = ?loop_config.working_dir,
                    "Loop mode applied"
                );
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

        // Track tool latency (v1.0) and role classification (v1.2)
        match event.event.as_str() {
            "PreToolUse" => {
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
                                    let status = extract_task_status(&event.tool_input);
                                    if status.as_deref() == Some("in_progress") {
                                        // Worker claiming a task
                                        agent.current_task_id = Some(task_id.clone());
                                    } else if status.as_deref() == Some("completed") {
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
                                        status = ?status,
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

        // v0.9.0 Loop Mode: Handle Stop events for loop-enabled agents
        if event.event == "Stop" && agent.loop_mode == LoopMode::Active {
            agent.loop_iteration += 1;

            // Track stop reason for stall detection (keep last 5)
            if let Some(reason) = &event.reason {
                agent.loop_last_reasons.push_back(reason.clone());
                while agent.loop_last_reasons.len() > 5 {
                    agent.loop_last_reasons.pop_front();
                }
            }

            let reason_str = event.reason.as_deref().unwrap_or("");

            // Check circuit breakers
            if agent.loop_iteration >= agent.loop_max {
                // Max iterations reached
                agent.loop_mode = LoopMode::Complete;
                tracing::info!(
                    pane_id = %pane_id,
                    iteration = agent.loop_iteration,
                    max = agent.loop_max,
                    "Loop complete: max iterations reached"
                );
            } else if !agent.loop_stop_word.is_empty()
                && reason_str
                    .to_uppercase()
                    .contains(&agent.loop_stop_word.to_uppercase())
            {
                // Stop word detected
                agent.loop_mode = LoopMode::Complete;
                tracing::info!(
                    pane_id = %pane_id,
                    stop_word = %agent.loop_stop_word,
                    reason = %reason_str,
                    "Loop complete: stop word detected"
                );
            } else if super::loop_handling::is_stalled(&agent.loop_last_reasons) {
                // Stall detected (5+ identical reasons)
                agent.loop_mode = LoopMode::Stalled;

                // Track error pattern for potential auto-guardrail
                if let Some(ref loop_dir) = agent.loop_dir {
                    let error_msg = format!("Stalled: {}", reason_str);
                    if let Ok(added_guardrail) =
                        rehoboam_loop::track_error_pattern(loop_dir, &error_msg)
                    {
                        if added_guardrail {
                            tracing::info!(
                                pane_id = %pane_id,
                                "Auto-added guardrail for stall pattern"
                            );
                        }
                    }
                    let _ = rehoboam_loop::log_session_transition(
                        loop_dir,
                        "working",
                        "stalled",
                        Some(reason_str),
                    );
                }

                // Kill stalled pane to prevent orphaned sessions
                if pane_id.starts_with('%') {
                    // Send Ctrl+C first for clean shutdown
                    let _ = TmuxController::send_interrupt(&pane_id);
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    if let Err(e) = TmuxController::kill_pane(&pane_id) {
                        tracing::warn!(
                            pane_id = %pane_id,
                            error = %e,
                            "Failed to kill stalled pane"
                        );
                    } else {
                        tracing::info!(pane_id = %pane_id, "Killed stalled loop pane");
                    }
                }

                tracing::warn!(
                    pane_id = %pane_id,
                    iteration = agent.loop_iteration,
                    last_reason = %reason_str,
                    "Loop stalled: 5+ identical stop reasons"
                );
                notify::send(
                    "Loop Stalled",
                    &format!("{}: {} (pane killed)", agent.project, reason_str),
                    Some("Basso"),
                );
            } else {
                // Evaluate completion using Judge (Claude Code)
                if let Some(ref loop_dir) = agent.loop_dir {
                    match rehoboam_loop::judge_completion(loop_dir) {
                        Ok((decision, confidence, explanation)) => match decision {
                            rehoboam_loop::JudgeDecision::Complete => {
                                agent.loop_mode = LoopMode::Complete;
                                tracing::info!(
                                    pane_id = %pane_id,
                                    confidence = confidence,
                                    explanation = %explanation,
                                    "Task complete"
                                );
                                return false;
                            }
                            rehoboam_loop::JudgeDecision::Stalled => {
                                agent.loop_mode = LoopMode::Stalled;
                                tracing::warn!(
                                    pane_id = %pane_id,
                                    confidence = confidence,
                                    explanation = %explanation,
                                    "Task stalled"
                                );
                                notify::send(
                                    "Loop Stalled",
                                    &format!("{}: {}", agent.project, explanation),
                                    Some("Basso"),
                                );
                                return false;
                            }
                            rehoboam_loop::JudgeDecision::Continue => {
                                tracing::debug!(
                                    pane_id = %pane_id,
                                    confidence = confidence,
                                    "Continuing loop"
                                );
                                // NOTE: Auto-spawn workers removed - TeammateTool handles team spawning
                            }
                        },
                        Err(e) => {
                            tracing::warn!(
                                pane_id = %pane_id,
                                error = %e,
                                "Completion check failed, continuing"
                            );
                        }
                    }
                }

                // Continue loop
                // Check agent type to determine how to continue
                if agent.is_sprite {
                    // Sprite agents: loop continuation handled async via SpriteController
                    // The app.rs handle_event will pick this up and send via WebSocket
                    tracing::info!(
                        pane_id = %pane_id,
                        iteration = agent.loop_iteration,
                        max = agent.loop_max,
                        "Sprite loop continuing (async)"
                    );
                } else if pane_id.starts_with('%') {
                    // Tmux panes: check if proper Rehoboam mode (fresh sessions)
                    // Clone loop_dir to avoid borrow conflict with mutable agent
                    let loop_dir_clone = agent.loop_dir.clone();
                    if let Some(loop_dir) = loop_dir_clone {
                        // Acquire respawn lock to prevent concurrent spawns
                        let _guard = match RESPAWN_LOCK.try_lock() {
                            Ok(guard) => guard,
                            Err(_) => {
                                tracing::debug!(
                                    pane_id = %pane_id,
                                    "Respawn already in progress, skipping"
                                );
                                return true; // Don't error, just skip
                            }
                        };

                        // Proper Rehoboam loop: spawn fresh session
                        match spawn_fresh_rehoboam_session(&pane_id, &loop_dir, agent) {
                            Ok(new_pane_id) => {
                                tracing::info!(
                                    old_pane = %pane_id,
                                    new_pane = %new_pane_id,
                                    iteration = agent.loop_iteration,
                                    "Rehoboam loop: spawned fresh session"
                                );
                                // Update pane_id if it changed
                                if new_pane_id != pane_id {
                                    agent.pane_id = new_pane_id;
                                }
                            }
                            Err(e) => {
                                // Track error pattern for potential auto-guardrail
                                let error_msg = format!("Spawn failed: {}", e);
                                let _ = rehoboam_loop::track_error_pattern(&loop_dir, &error_msg);
                                let _ = rehoboam_loop::log_session_transition(
                                    &loop_dir,
                                    "respawning",
                                    "error",
                                    Some(&error_msg),
                                );

                                tracing::error!(
                                    pane_id = %pane_id,
                                    error = %e,
                                    "Failed to spawn fresh Rehoboam session"
                                );
                                agent.loop_mode = LoopMode::Stalled;
                                notify::send(
                                    "Rehoboam Error",
                                    &format!("{}: {}", agent.project, e),
                                    Some("Basso"),
                                );
                            }
                        }
                    } else {
                        // Legacy loop mode: send Enter (same session)
                        if let Err(e) = TmuxController::send_enter(&pane_id) {
                            tracing::error!(
                                pane_id = %pane_id,
                                error = %e,
                                "Failed to send Enter for loop continuation"
                            );
                        } else {
                            tracing::info!(
                                pane_id = %pane_id,
                                iteration = agent.loop_iteration,
                                max = agent.loop_max,
                                "Loop continuing: sent Enter (legacy mode)"
                            );
                        }
                    }
                }
            }
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
