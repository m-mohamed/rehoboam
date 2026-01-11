mod agent;

pub use agent::{Agent, LoopMode, Status, Subagent};

use crate::config::{MAX_AGENTS, MAX_EVENTS, MAX_SPARKLINE_POINTS};
use crate::event::{EventSource, HookEvent};
use crate::notify;
use crate::tmux::TmuxController;
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{SystemTime, UNIX_EPOCH};

/// Timeout for Working → Idle transition (seconds)
/// Reduced from 30s to 10s for faster responsiveness
const IDLE_TIMEOUT_SECS: i64 = 10;

/// Timeout for removing stale sessions (seconds)
const STALE_TIMEOUT_SECS: i64 = 300; // 5 minutes

/// Number of status columns in Kanban view
pub const NUM_COLUMNS: usize = 4;

/// Loop configuration for pending spawn
#[derive(Debug, Clone)]
pub struct LoopConfig {
    /// Maximum iterations before stopping
    pub max_iterations: u32,
    /// Stop word to detect completion
    pub stop_word: String,
}

/// Application state
#[derive(Debug)]
pub struct AppState {
    /// Active agents indexed by pane_id
    pub agents: HashMap<String, Agent>,
    /// Recent events for the event log
    pub events: VecDeque<HookEvent>,
    /// Currently selected column (0=Attention, 1=Working, 2=Compact, 3=Idle)
    pub selected_column: usize,
    /// Currently selected card index within the column
    pub selected_card: usize,
    /// Cached status counts: [attention, working, compacting, idle]
    pub status_counts: [usize; NUM_COLUMNS],
    /// Set of selected pane_ids for bulk operations
    pub selected_agents: HashSet<String>,
    /// Pending loop configs for newly spawned agents (pane_id -> config)
    pub pending_loop_configs: HashMap<String, LoopConfig>,
    /// Set of sprite agent IDs (for quick lookup)
    pub sprite_agent_ids: HashSet<String>,
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
        }
    }
}

/// Map status to column index
fn status_to_column(status: &Status) -> usize {
    match status {
        Status::Attention(_) => 0,
        Status::Working => 1,
        Status::Compacting => 2,
        Status::Idle => 3,
    }
}

/// Get human-readable name for column index
fn column_name(col: usize) -> &'static str {
    match col {
        0 => "attention",
        1 => "working",
        2 => "compacting",
        3 => "idle",
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
    pub fn process_event(&mut self, event: HookEvent) {
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

        // Evict oldest idle agent if at capacity and adding new agent
        if is_new_agent && self.agents.len() >= MAX_AGENTS {
            self.evict_oldest_idle();
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
                Agent::new_sprite(sprite_id.clone().unwrap_or_else(|| pane_id.clone()), project)
            } else {
                Agent::new(pane_id.clone(), project)
            }
        });

        // Update agent state
        agent.project = event.project.clone();
        agent.status = Status::from_str(&event.status, event.attention_type.as_deref());
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
                tracing::info!(
                    pane_id = %pane_id,
                    max = loop_config.max_iterations,
                    stop_word = %loop_config.stop_word,
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
                        start_time: event.timestamp,
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
                    if let Some(subagent) = agent.subagents.iter_mut().find(|s| &s.id == subagent_id)
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
                // Continue loop: send Enter to re-prompt
                // Check agent type to determine how to send input
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
                    // Tmux panes: send Enter directly
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
                            "Loop continuing: sent Enter"
                        );
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
            Status::Attention(_) => 0.8,
            Status::Compacting => 0.6,
            Status::Idle => 0.1,
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
    }

    /// Evict the oldest idle agent to make room for new ones
    fn evict_oldest_idle(&mut self) {
        // Find oldest idle agent by last_update
        let oldest_idle = self
            .agents
            .iter()
            .filter(|(_, a)| matches!(a.status, Status::Idle))
            .min_by_key(|(_, a)| a.last_update)
            .map(|(id, _)| id.clone());

        if let Some(pane_id) = oldest_idle {
            self.status_counts[3] = self.status_counts[3].saturating_sub(1); // Idle is column 3
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
    /// - Working → Idle after IDLE_TIMEOUT_SECS (30s) of no events
    /// - Remove stale sessions after STALE_TIMEOUT_SECS (5 min) of no events
    pub fn tick(&mut self) {
        let now = current_timestamp();
        let mut to_remove: Vec<String> = Vec::new();
        let mut idle_transitions: Vec<String> = Vec::new();

        for (pane_id, agent) in self.agents.iter() {
            let elapsed = now - agent.last_update;

            // Remove stale sessions (5 minutes of no events)
            if elapsed > STALE_TIMEOUT_SECS {
                to_remove.push(pane_id.clone());
                continue;
            }

            // Working → Idle after timeout, BUT NOT if:
            // 1. A tool is currently running (between PreToolUse and PostToolUse)
            // 2. Claude is actively responding (between UserPromptSubmit and Stop)
            if matches!(agent.status, Status::Working)
                && elapsed > IDLE_TIMEOUT_SECS
                && agent.current_tool.is_none()
                && !agent.in_response
            {
                idle_transitions.push(pane_id.clone());
            }
        }

        // Apply idle transitions
        for pane_id in idle_transitions {
            if let Some(agent) = self.agents.get_mut(&pane_id) {
                let old_col = status_to_column(&agent.status);
                agent.status = Status::Idle;
                let new_col = status_to_column(&agent.status);

                // Update status counts
                self.status_counts[old_col] = self.status_counts[old_col].saturating_sub(1);
                self.status_counts[new_col] += 1;

                tracing::info!(
                    pane_id = %pane_id,
                    project = %agent.project,
                    elapsed_secs = %(now - agent.last_update),
                    "Timeout: Working → Idle"
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
    /// Returns 4 vectors: [Attention, Working, Compacting, Idle]
    /// Each sorted by project name for consistency
    pub fn agents_by_column(&self) -> [Vec<&Agent>; NUM_COLUMNS] {
        let mut columns: [Vec<&Agent>; NUM_COLUMNS] = Default::default();
        for agent in self.agents.values() {
            let col = match &agent.status {
                Status::Attention(_) => 0,
                Status::Working => 1,
                Status::Compacting => 2,
                Status::Idle => 3,
            };
            columns[col].push(agent);
        }
        // Sort each column by project name for consistent ordering
        for col in &mut columns {
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

    /// Check if an agent is selected
    #[allow(dead_code)]
    pub fn is_selected(&self, pane_id: &str) -> bool {
        self.selected_agents.contains(pane_id)
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

    /// Enable loop mode on an agent
    ///
    /// Called when spawning an agent in loop mode or enabling loop on existing agent.
    pub fn enable_loop_mode(&mut self, pane_id: &str, max_iterations: u32, stop_word: &str) {
        if let Some(agent) = self.agents.get_mut(pane_id) {
            agent.loop_mode = LoopMode::Active;
            agent.loop_iteration = 0;
            agent.loop_max = max_iterations;
            agent.loop_stop_word = stop_word.to_string();
            agent.loop_last_reasons.clear();
            tracing::info!(
                pane_id = %pane_id,
                max = max_iterations,
                stop_word = %stop_word,
                "Loop mode enabled"
            );
        }
    }

    /// Register a pending loop config for a newly spawned agent
    ///
    /// When the agent sends its first hook event, the config will be applied.
    pub fn register_loop_config(&mut self, pane_id: &str, max_iterations: u32, stop_word: &str) {
        self.pending_loop_configs.insert(
            pane_id.to_string(),
            LoopConfig {
                max_iterations,
                stop_word: stop_word.to_string(),
            },
        );
        tracing::info!(
            pane_id = %pane_id,
            max = max_iterations,
            stop_word = %stop_word,
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

    /// Check if an agent is a sprite agent
    pub fn is_sprite_agent(&self, pane_id: &str) -> bool {
        self.sprite_agent_ids.contains(pane_id)
    }

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

    /// Get all sprite agents
    pub fn sprite_agents(&self) -> impl Iterator<Item = &Agent> {
        self.agents
            .values()
            .filter(|a| a.is_sprite)
    }
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
