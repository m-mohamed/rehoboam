mod agent;

pub use agent::{Agent, AttentionType, LoopMode, Status, Subagent};

use crate::config::{MAX_AGENTS, MAX_EVENTS, MAX_SPARKLINE_POINTS};
use crate::event::{EventSource, HookEvent};
use crate::notify;
use crate::ralph;
use crate::tmux::TmuxController;
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{SystemTime, UNIX_EPOCH};

/// Timeout for Working → Attention(Waiting) transition (seconds)
/// Increased from 10s to 60s - Claude often thinks for 10-30s between tool calls
/// The in_response guard prevents false timeouts during active responses
const WAITING_TIMEOUT_SECS: i64 = 60;

/// Timeout for removing stale sessions (seconds)
const STALE_TIMEOUT_SECS: i64 = 300; // 5 minutes

/// Number of status columns in Kanban view (Attention, Working, Compacting)
pub const NUM_COLUMNS: usize = 3;

/// Loop configuration for pending spawn
#[derive(Debug, Clone)]
pub struct LoopConfig {
    /// Maximum iterations before stopping
    pub max_iterations: u32,
    /// Stop word to detect completion
    pub stop_word: String,
    /// Path to .ralph/ directory for proper Ralph loops (fresh sessions)
    pub ralph_dir: Option<std::path::PathBuf>,
}

/// Application state
#[derive(Debug)]
pub struct AppState {
    /// Active agents indexed by pane_id
    pub agents: HashMap<String, Agent>,
    /// Recent events for the event log
    pub events: VecDeque<HookEvent>,
    /// Currently selected column (0=Attention, 1=Working, 2=Compacting)
    pub selected_column: usize,
    /// Currently selected card index within the column
    pub selected_card: usize,
    /// Cached status counts: [attention, working, compacting]
    pub status_counts: [usize; NUM_COLUMNS],
    /// Set of selected pane_ids for bulk operations
    pub selected_agents: HashSet<String>,
    /// Pending loop configs for newly spawned agents (pane_id -> config)
    pub pending_loop_configs: HashMap<String, LoopConfig>,
    /// Set of sprite agent IDs (for quick lookup)
    pub sprite_agent_ids: HashSet<String>,
    /// Set of currently connected sprite IDs
    pub connected_sprites: HashSet<String>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            agents: HashMap::new(),
            events: VecDeque::new(),
            selected_column: 0,
            selected_card: 0,
            status_counts: [0; NUM_COLUMNS],
            selected_agents: HashSet::new(),
            pending_loop_configs: HashMap::new(),
            sprite_agent_ids: HashSet::new(),
            connected_sprites: HashSet::new(),
        }
    }
}

/// Map status to column index
fn status_to_column(status: &Status) -> usize {
    match status {
        Status::Attention(_) => 0,
        Status::Working => 1,
        Status::Compacting => 2,
    }
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

/// Check if loop is stalled (5+ consecutive identical stop reasons)
///
/// Stall detection prevents infinite loops on repeating errors.
/// If the last 5 stop reasons are identical, the agent is stuck.
fn is_stalled(reasons: &VecDeque<String>) -> bool {
    if reasons.len() < 5 {
        return false;
    }
    if let Some(last) = reasons.back() {
        reasons.iter().rev().take(5).all(|r| r == last)
    } else {
        false
    }
}

impl AppState {
    pub fn new() -> Self {
        Self::default()
    }

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
        if is_new_agent && self.agents.len() >= MAX_AGENTS {
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

        // Priority-aware status update: don't let Working override blocking Attention states
        let new_status = Status::from_str(&event.status, event.attention_type.as_deref());
        let should_update = match (&agent.status, &new_status) {
            // Current status is blocking Attention (Permission or Input)
            // Don't let Working or lower-priority Attention override it
            (Status::Attention(current_attn), Status::Working)
                if matches!(current_attn, AttentionType::Permission | AttentionType::Input) =>
            {
                false
            }
            // Current is Attention, new is also Attention - use priority
            (Status::Attention(current_attn), Status::Attention(new_attn)) => {
                // Only update if new attention has equal or higher priority (lower number)
                new_attn.priority() <= current_attn.priority()
            }
            // All other cases - allow the update
            _ => true,
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
                agent.ralph_dir = loop_config.ralph_dir.clone();
                tracing::info!(
                    pane_id = %pane_id,
                    max = loop_config.max_iterations,
                    stop_word = %loop_config.stop_word,
                    ralph_dir = ?loop_config.ralph_dir,
                    "Loop mode applied from spawn config"
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

        // Track tool latency (v1.0)
        match event.event.as_str() {
            "PreToolUse" => {
                if let Some(tool) = &event.tool_name {
                    agent.start_tool(tool, event.tool_use_id.as_deref(), event.timestamp);
                    tracing::debug!(
                        pane_id = %pane_id,
                        tool = %tool,
                        "Tool started"
                    );
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
            // v0.9.0: Subagent tracking
            "SubagentStart" => {
                if let Some(subagent_id) = &event.subagent_id {
                    let description = event
                        .description
                        .clone()
                        .unwrap_or_else(|| "subagent".to_string());
                    agent.subagents.push(Subagent {
                        id: subagent_id.clone(),
                        description: description.clone(),
                        status: "running".to_string(),
                        duration_ms: None,
                    });
                    tracing::info!(
                        pane_id = %pane_id,
                        subagent_id = %subagent_id,
                        description = %description,
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
            } else if is_stalled(&agent.loop_last_reasons) {
                // Stall detected (5+ identical reasons)
                agent.loop_mode = LoopMode::Stalled;

                // Track error pattern for potential auto-guardrail
                if let Some(ref ralph_dir) = agent.ralph_dir {
                    let error_msg = format!("Stalled: {}", reason_str);
                    if let Ok(added_guardrail) = ralph::track_error_pattern(ralph_dir, &error_msg) {
                        if added_guardrail {
                            tracing::info!(
                                pane_id = %pane_id,
                                "Auto-added guardrail for stall pattern"
                            );
                        }
                    }
                    let _ = ralph::log_session_transition(
                        ralph_dir,
                        "working",
                        "stalled",
                        Some(reason_str),
                    );
                }

                tracing::warn!(
                    pane_id = %pane_id,
                    iteration = agent.loop_iteration,
                    last_reason = %reason_str,
                    "Loop stalled: 5+ identical stop reasons"
                );
                notify::send(
                    "Loop Stalled",
                    &format!("{}: {}", agent.project, reason_str),
                    Some("Basso"),
                );
            } else {
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
                    // Tmux panes: check if proper Ralph mode (fresh sessions)
                    // Clone ralph_dir to avoid borrow conflict with mutable agent
                    let ralph_dir_clone = agent.ralph_dir.clone();
                    if let Some(ralph_dir) = ralph_dir_clone {
                        // Proper Ralph loop: spawn fresh session
                        match spawn_fresh_ralph_session(&pane_id, &ralph_dir, agent) {
                            Ok(new_pane_id) => {
                                tracing::info!(
                                    old_pane = %pane_id,
                                    new_pane = %new_pane_id,
                                    iteration = agent.loop_iteration,
                                    "Ralph loop: spawned fresh session"
                                );
                                // Update pane_id if it changed
                                if new_pane_id != pane_id {
                                    agent.pane_id = new_pane_id;
                                }
                            }
                            Err(e) => {
                                // Track error pattern for potential auto-guardrail
                                let error_msg = format!("Spawn failed: {}", e);
                                let _ = ralph::track_error_pattern(&ralph_dir, &error_msg);
                                let _ = ralph::log_session_transition(
                                    &ralph_dir,
                                    "respawning",
                                    "error",
                                    Some(&error_msg),
                                );

                                tracing::error!(
                                    pane_id = %pane_id,
                                    error = %e,
                                    "Failed to spawn fresh Ralph session"
                                );
                                agent.loop_mode = LoopMode::Stalled;
                                notify::send(
                                    "Ralph Error",
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

    /// Evict the oldest waiting agent to make room for new ones
    fn evict_oldest_waiting(&mut self) {
        // Find oldest Attention(Waiting) agent by last_update
        let oldest_waiting = self
            .agents
            .iter()
            .filter(|(_, a)| matches!(a.status, Status::Attention(AttentionType::Waiting)))
            .min_by_key(|(_, a)| a.last_update)
            .map(|(id, _)| id.clone());

        if let Some(pane_id) = oldest_waiting {
            self.status_counts[0] = self.status_counts[0].saturating_sub(1); // Attention is column 0
            self.agents.remove(&pane_id);
        } else {
            // No idle agents, evict oldest of any status
            let oldest = self
                .agents
                .iter()
                .min_by_key(|(_, a)| a.last_update)
                .map(|(id, a)| (id.clone(), status_to_column(&a.status)));

            if let Some((pane_id, col)) = oldest {
                self.status_counts[col] = self.status_counts[col].saturating_sub(1);
                self.agents.remove(&pane_id);
            }
        }
    }

    /// Periodic tick for timeout-based state transitions
    ///
    /// Handles:
    /// - Working → Attention(Waiting) after WAITING_TIMEOUT_SECS (60s) of no events
    /// - Remove stale sessions after STALE_TIMEOUT_SECS (5 min) of no events
    pub fn tick(&mut self) {
        let now = current_timestamp();
        let mut to_remove: Vec<String> = Vec::new();
        let mut waiting_transitions: Vec<String> = Vec::new();

        for (pane_id, agent) in &self.agents {
            let elapsed = now - agent.last_update;

            // Remove stale sessions (5 minutes of no events)
            if elapsed > STALE_TIMEOUT_SECS {
                to_remove.push(pane_id.clone());
                continue;
            }

            // Working → Attention(Waiting) after timeout, BUT NOT if:
            // 1. A tool is currently running (between PreToolUse and PostToolUse)
            // 2. Claude is actively responding (between UserPromptSubmit and Stop)
            if matches!(agent.status, Status::Working) {
                // Debug: log timeout check conditions
                if elapsed > 10 {
                    // Only log after 10s to reduce noise
                    tracing::debug!(
                        pane_id = %pane_id,
                        elapsed_secs = elapsed,
                        in_response = agent.in_response,
                        current_tool = ?agent.current_tool,
                        timeout_threshold = WAITING_TIMEOUT_SECS,
                        "Timeout check"
                    );
                }

                if elapsed > WAITING_TIMEOUT_SECS && agent.current_tool.is_none() && !agent.in_response
                {
                    waiting_transitions.push(pane_id.clone());
                }
            }
        }

        // Apply waiting transitions
        for pane_id in waiting_transitions {
            if let Some(agent) = self.agents.get_mut(&pane_id) {
                let old_col = status_to_column(&agent.status);
                agent.status = Status::Attention(AttentionType::Waiting);
                let new_col = status_to_column(&agent.status);

                // Update status counts
                self.status_counts[old_col] = self.status_counts[old_col].saturating_sub(1);
                self.status_counts[new_col] += 1;

                tracing::info!(
                    pane_id = %pane_id,
                    project = %agent.project,
                    elapsed_secs = %(now - agent.last_update),
                    "Timeout: Working → Attention(Waiting)"
                );
            }
        }

        // Remove stale sessions
        for pane_id in to_remove {
            if let Some(agent) = self.agents.get(&pane_id) {
                let col = status_to_column(&agent.status);
                self.status_counts[col] = self.status_counts[col].saturating_sub(1);
                tracing::info!(
                    pane_id = %pane_id,
                    project = %agent.project,
                    "Removed stale session (5 min timeout)"
                );
            }
            self.agents.remove(&pane_id);
        }
    }

    /// Get agents grouped by status column
    ///
    /// Returns 3 vectors: [Attention, Working, Compacting]
    /// Attention column sorted by AttentionType priority, then by project name
    pub fn agents_by_column(&self) -> [Vec<&Agent>; NUM_COLUMNS] {
        let mut columns: [Vec<&Agent>; NUM_COLUMNS] = Default::default();
        for agent in self.agents.values() {
            let col = match &agent.status {
                Status::Attention(_) => 0,
                Status::Working => 1,
                Status::Compacting => 2,
            };
            columns[col].push(agent);
        }
        // Sort Attention column by AttentionType priority, then by project name
        columns[0].sort_by(|a, b| {
            match (&a.status, &b.status) {
                (Status::Attention(a_type), Status::Attention(b_type)) => {
                    a_type.priority().cmp(&b_type.priority())
                        .then_with(|| a.project.cmp(&b.project))
                }
                _ => a.project.cmp(&b.project),
            }
        });
        // Sort other columns by project name for consistent ordering
        for col in &mut columns[1..] {
            col.sort_by(|a, b| a.project.cmp(&b.project));
        }
        columns
    }

    /// Get agents grouped by project name
    ///
    /// Returns a vector of (project_name, agents) tuples, sorted by project name.
    /// Within each project, agents are sorted by status priority (attention first).
    pub fn agents_by_project(&self) -> Vec<(String, Vec<&Agent>)> {
        let mut projects: HashMap<String, Vec<&Agent>> = HashMap::new();

        for agent in self.agents.values() {
            projects
                .entry(agent.project.clone())
                .or_default()
                .push(agent);
        }

        // Sort agents within each project by status priority
        for agents in projects.values_mut() {
            agents.sort_by(|a, b| a.status.priority().cmp(&b.status.priority()));
        }

        // Convert to sorted vector of tuples
        let mut result: Vec<(String, Vec<&Agent>)> = projects.into_iter().collect();
        result.sort_by(|a, b| a.0.cmp(&b.0));
        result
    }

    /// Move to next card in current column
    pub fn next_card(&mut self) {
        let columns = self.agents_by_column();
        let col_len = columns[self.selected_column].len();
        if col_len > 0 {
            self.selected_card = (self.selected_card + 1) % col_len;
        }
    }

    /// Move to previous card in current column
    pub fn previous_card(&mut self) {
        let columns = self.agents_by_column();
        let col_len = columns[self.selected_column].len();
        if col_len > 0 {
            self.selected_card = (self.selected_card + col_len - 1) % col_len;
        }
    }

    /// Move to column on the left
    pub fn move_column_left(&mut self) {
        self.selected_column = (self.selected_column + NUM_COLUMNS - 1) % NUM_COLUMNS;
        // Clamp card selection to new column's bounds
        self.clamp_card_selection();
    }

    /// Move to column on the right
    pub fn move_column_right(&mut self) {
        self.selected_column = (self.selected_column + 1) % NUM_COLUMNS;
        // Clamp card selection to new column's bounds
        self.clamp_card_selection();
    }

    /// Clamp card selection to valid range for current column
    fn clamp_card_selection(&mut self) {
        let columns = self.agents_by_column();
        let col_len = columns[self.selected_column].len();
        if col_len == 0 {
            self.selected_card = 0;
        } else if self.selected_card >= col_len {
            self.selected_card = col_len - 1;
        }
    }

    /// Get currently selected agent
    pub fn selected_agent(&self) -> Option<&Agent> {
        let columns = self.agents_by_column();
        columns[self.selected_column]
            .get(self.selected_card)
            .copied()
    }

    /// Toggle selection of the currently focused agent
    pub fn toggle_selection(&mut self) {
        if let Some(agent) = self.selected_agent() {
            let pane_id = agent.pane_id.clone();
            if self.selected_agents.contains(&pane_id) {
                self.selected_agents.remove(&pane_id);
            } else {
                self.selected_agents.insert(pane_id);
            }
        }
    }

    /// Clear all selections
    pub fn clear_selection(&mut self) {
        self.selected_agents.clear();
    }

    /// Get list of selected agent pane_ids (tmux only)
    pub fn selected_tmux_panes(&self) -> Vec<String> {
        self.selected_agents
            .iter()
            .filter(|id| id.starts_with('%'))
            .cloned()
            .collect()
    }

    // v0.9.0 Loop Mode Methods

    /// Register a pending loop config for a newly spawned agent
    ///
    /// When the agent sends its first hook event, the config will be applied.
    /// If `ralph_dir` is Some, the agent will use proper Ralph mode (fresh sessions).
    pub fn register_loop_config(
        &mut self,
        pane_id: &str,
        max_iterations: u32,
        stop_word: &str,
        ralph_dir: Option<std::path::PathBuf>,
    ) {
        self.pending_loop_configs.insert(
            pane_id.to_string(),
            LoopConfig {
                max_iterations,
                stop_word: stop_word.to_string(),
                ralph_dir: ralph_dir.clone(),
            },
        );
        tracing::info!(
            pane_id = %pane_id,
            max = max_iterations,
            stop_word = %stop_word,
            ralph_dir = ?ralph_dir,
            "Registered pending loop config"
        );
    }

    /// Cancel loop mode (X key)
    ///
    /// Stops sending Enter on Stop events. Agent continues to run but won't auto-continue.
    pub fn cancel_loop(&mut self, pane_id: &str) {
        if let Some(agent) = self.agents.get_mut(pane_id) {
            if agent.loop_mode == LoopMode::Active {
                agent.loop_mode = LoopMode::None;
                tracing::info!(
                    pane_id = %pane_id,
                    iteration = agent.loop_iteration,
                    "Loop cancelled"
                );
            }
        }
    }

    /// Restart loop mode (R key)
    ///
    /// Resets iteration counter and resumes sending Enter on Stop events.
    pub fn restart_loop(&mut self, pane_id: &str) {
        if let Some(agent) = self.agents.get_mut(pane_id) {
            if matches!(
                agent.loop_mode,
                LoopMode::Stalled | LoopMode::Complete | LoopMode::None
            ) {
                agent.loop_mode = LoopMode::Active;
                agent.loop_iteration = 0;
                agent.loop_last_reasons.clear();
                tracing::info!(
                    pane_id = %pane_id,
                    max = agent.loop_max,
                    "Loop restarted"
                );
            }
        }
    }

    /// Get the pane_id of the currently selected agent
    pub fn selected_pane_id(&self) -> Option<String> {
        self.selected_agent().map(|a| a.pane_id.clone())
    }

    // v0.10.0 Sprite Methods

    /// Get list of selected sprite agent IDs
    pub fn selected_sprite_agents(&self) -> Vec<String> {
        self.selected_agents
            .iter()
            .filter(|id| self.sprite_agent_ids.contains(*id))
            .cloned()
            .collect()
    }

    /// Get count of sprite agents
    pub fn sprite_agent_count(&self) -> usize {
        self.sprite_agent_ids.len()
    }

    /// Mark a sprite as connected
    pub fn sprite_connected(&mut self, sprite_id: &str) {
        self.connected_sprites.insert(sprite_id.to_string());
    }

    /// Mark a sprite as disconnected
    pub fn sprite_disconnected(&mut self, sprite_id: &str) {
        self.connected_sprites.remove(sprite_id);
    }

    /// Get count of connected sprites
    pub fn connected_sprite_count(&self) -> usize {
        self.connected_sprites.len()
    }

    /// Set the working directory for an agent (for git operations)
    pub fn set_agent_working_dir(&mut self, pane_id: &str, working_dir: std::path::PathBuf) {
        if let Some(agent) = self.agents.get_mut(pane_id) {
            agent.working_dir = Some(working_dir);
        }
    }
}

/// Spawn a fresh Ralph session in the given pane
///
/// This is the core of proper Ralph loops:
/// 1. Increment iteration counter in state.json
/// 2. Check stop word in progress.md
/// 3. Build iteration prompt with current state
/// 4. Kill old pane, spawn fresh Claude session
///
/// Returns the new pane_id (may be different from old one)
fn spawn_fresh_ralph_session(
    pane_id: &str,
    ralph_dir: &std::path::Path,
    agent: &mut Agent,
) -> color_eyre::eyre::Result<String> {
    use color_eyre::eyre::WrapErr;

    // Log session transition: iteration ending
    let _ =
        ralph::log_session_transition(ralph_dir, "working", "stopping", Some("iteration ending"));

    // Get iteration duration before incrementing
    let duration = ralph::get_iteration_duration(ralph_dir);

    // 1. Increment iteration counter
    let new_iteration =
        ralph::increment_iteration(ralph_dir).wrap_err("Failed to increment Ralph iteration")?;
    agent.loop_iteration = new_iteration;

    // 2. Check completion (stop word OR promise tag)
    let (is_complete, completion_reason) =
        ralph::check_completion(ralph_dir, &agent.loop_stop_word)
            .wrap_err("Failed to check completion")?;

    if is_complete {
        // Log activity for completed iteration
        let _ = ralph::log_activity(
            ralph_dir,
            new_iteration,
            duration,
            None,
            &format!("complete:{}", completion_reason),
        );

        // Create final git checkpoint
        let _ = ralph::create_git_checkpoint(ralph_dir);
        let _ = ralph::log_session_transition(
            ralph_dir,
            "stopping",
            "complete",
            Some(&completion_reason),
        );

        agent.loop_mode = LoopMode::Complete;
        tracing::info!(
            pane_id = %pane_id,
            iteration = new_iteration,
            reason = %completion_reason,
            "Ralph loop complete"
        );
        notify::send(
            "Ralph Complete",
            &format!(
                "{}: {} iterations ({})",
                agent.project, new_iteration, completion_reason
            ),
            Some("Glass"),
        );
        return Ok(pane_id.to_string());
    }

    // 3. Check max iterations
    if ralph::check_max_iterations(ralph_dir).wrap_err("Failed to check max iterations")? {
        // Log activity
        let _ = ralph::log_activity(ralph_dir, new_iteration, duration, None, "max_iterations");

        // Create git checkpoint
        let _ = ralph::create_git_checkpoint(ralph_dir);
        let _ = ralph::log_session_transition(ralph_dir, "stopping", "max_reached", None);

        agent.loop_mode = LoopMode::Complete;
        tracing::info!(
            pane_id = %pane_id,
            iteration = new_iteration,
            max = agent.loop_max,
            "Ralph loop complete: max iterations reached"
        );
        notify::send(
            "Ralph Max Reached",
            &format!("{}: {} iterations", agent.project, new_iteration),
            Some("Basso"),
        );
        return Ok(pane_id.to_string());
    }

    // Log activity for continuing iteration
    let _ = ralph::log_activity(ralph_dir, new_iteration, duration, None, "continuing");

    // 4. Create git checkpoint before respawning
    let _ = ralph::create_git_checkpoint(ralph_dir);

    // 5. Build iteration prompt
    let prompt_file =
        ralph::build_iteration_prompt(ralph_dir).wrap_err("Failed to build iteration prompt")?;

    // 6. Get project directory for respawn
    let project_dir = ralph_dir
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| ".".to_string());

    // Log session transition: respawning
    let _ = ralph::log_session_transition(
        ralph_dir,
        "stopping",
        "respawning",
        Some(&format!("iteration {}", new_iteration + 1)),
    );

    // 7. Send Ctrl+C to ensure clean shutdown, then kill pane
    let _ = TmuxController::send_interrupt(pane_id);
    std::thread::sleep(std::time::Duration::from_millis(100));

    if let Err(e) = TmuxController::kill_pane(pane_id) {
        tracing::warn!(
            pane_id = %pane_id,
            error = %e,
            "Failed to kill old pane (may already be gone)"
        );
    }

    // 8. Respawn fresh Claude session
    let new_pane_id = TmuxController::respawn_claude(&project_dir, &prompt_file)
        .wrap_err("Failed to respawn Claude session")?;

    // 9. Mark iteration start time for next iteration
    let _ = ralph::mark_iteration_start(ralph_dir);
    let _ = ralph::log_session_transition(ralph_dir, "respawning", "working", Some(&new_pane_id));

    tracing::info!(
        old_pane = %pane_id,
        new_pane = %new_pane_id,
        iteration = new_iteration,
        prompt_file = %prompt_file,
        "Spawned fresh Ralph session"
    );

    Ok(new_pane_id)
}

fn current_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::HookEvent;

    /// Create a test hook event with minimal required fields
    fn make_event(event: &str, status: &str, pane_id: &str, project: &str) -> HookEvent {
        HookEvent {
            event: event.to_string(),
            status: status.to_string(),
            attention_type: None,
            pane_id: pane_id.to_string(),
            project: project.to_string(),
            timestamp: current_timestamp(),
            session_id: None,
            tool_name: None,
            tool_input: None,
            tool_use_id: None,
            reason: None,
            subagent_id: None,
            description: None,
            subagent_duration_ms: None,
            source: crate::event::EventSource::Local,
        }
    }

    #[test]
    fn test_process_event_new_agent() {
        let mut state = AppState::new();
        let event = make_event("SessionStart", "working", "%0", "test-project");

        let changed = state.process_event(event);

        assert!(changed);
        assert_eq!(state.agents.len(), 1);
        assert_eq!(state.status_counts[1], 1); // Working is column 1
        assert!(state.agents.contains_key("%0"));

        let agent = state.agents.get("%0").unwrap();
        assert_eq!(agent.project, "test-project");
        assert!(matches!(agent.status, Status::Working));
    }

    #[test]
    fn test_status_counts_updated_on_transition() {
        let mut state = AppState::new();

        // Create an agent in working state
        let _ = state.process_event(make_event("SessionStart", "working", "%0", "test"));
        assert_eq!(state.status_counts[1], 1); // Working

        // Transition to idle (now Attention(Waiting) in column 0)
        let _ = state.process_event(make_event("Stop", "idle", "%0", "test"));
        assert_eq!(state.status_counts[1], 0); // Working now 0
        assert_eq!(state.status_counts[0], 1); // Attention (includes Waiting) now 1
    }

    #[test]
    fn test_agent_eviction_at_capacity() {
        use crate::config::MAX_AGENTS;
        let mut state = AppState::new();

        // Fill to capacity with idle agents
        let base_time = current_timestamp();
        for i in 0..MAX_AGENTS {
            let mut event = make_event("SessionStart", "idle", &format!("%{}", i), "test");
            // Vary timestamps so we have a clear oldest
            // %0 gets oldest timestamp, each subsequent agent gets newer
            event.timestamp = base_time + (i as i64);
            let _ = state.process_event(event);
        }

        assert_eq!(state.agents.len(), MAX_AGENTS);

        // Manually set last_update to match our timestamps
        // (process_event uses current_timestamp() for last_update)
        for i in 0..MAX_AGENTS {
            if let Some(agent) = state.agents.get_mut(&format!("%{}", i)) {
                agent.last_update = base_time + (i as i64);
            }
        }

        // Add one more - should evict oldest idle (%0)
        let _ = state.process_event(make_event("SessionStart", "working", "%new", "test"));

        assert_eq!(state.agents.len(), MAX_AGENTS);
        assert!(state.agents.contains_key("%new"));
        // %0 should have been evicted (oldest by last_update)
        assert!(!state.agents.contains_key("%0"));
    }

    #[test]
    fn test_session_end_removes_agent() {
        let mut state = AppState::new();

        // Create an agent
        let _ = state.process_event(make_event("SessionStart", "working", "%0", "test"));
        assert_eq!(state.agents.len(), 1);

        // End the session
        let _ = state.process_event(make_event("SessionEnd", "idle", "%0", "test"));
        assert_eq!(state.agents.len(), 0);
        assert_eq!(state.status_counts[1], 0); // Working
        assert_eq!(state.status_counts[0], 0); // Attention (includes Waiting)
    }

    #[test]
    fn test_is_stalled_empty() {
        let reasons: VecDeque<String> = VecDeque::new();
        assert!(!is_stalled(&reasons));
    }

    #[test]
    fn test_is_stalled_less_than_five() {
        let mut reasons = VecDeque::new();
        reasons.push_back("error".to_string());
        reasons.push_back("error".to_string());
        reasons.push_back("error".to_string());
        reasons.push_back("error".to_string());
        assert!(!is_stalled(&reasons)); // Only 4, need 5
    }

    #[test]
    fn test_is_stalled_five_identical() {
        let mut reasons = VecDeque::new();
        for _ in 0..5 {
            reasons.push_back("File not found".to_string());
        }
        assert!(is_stalled(&reasons));
    }

    #[test]
    fn test_is_stalled_five_different() {
        let mut reasons = VecDeque::new();
        reasons.push_back("error1".to_string());
        reasons.push_back("error2".to_string());
        reasons.push_back("error3".to_string());
        reasons.push_back("error4".to_string());
        reasons.push_back("error5".to_string());
        assert!(!is_stalled(&reasons));
    }

    #[test]
    fn test_is_stalled_last_five_identical_with_different_earlier() {
        let mut reasons = VecDeque::new();
        reasons.push_back("different".to_string());
        reasons.push_back("same".to_string());
        reasons.push_back("same".to_string());
        reasons.push_back("same".to_string());
        reasons.push_back("same".to_string());
        reasons.push_back("same".to_string());
        assert!(is_stalled(&reasons)); // Last 5 are "same"
    }
}
