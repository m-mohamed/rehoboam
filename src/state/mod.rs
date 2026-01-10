mod agent;

pub use agent::{Agent, Status};

use crate::config::{MAX_AGENTS, MAX_EVENTS, MAX_SPARKLINE_POINTS};
use crate::event::HookEvent;
use std::collections::{HashMap, VecDeque};
use std::time::{SystemTime, UNIX_EPOCH};

/// Timeout for Working → Idle transition (seconds)
/// Reduced from 30s to 10s for faster responsiveness
const IDLE_TIMEOUT_SECS: i64 = 10;

/// Timeout for removing stale sessions (seconds)
const STALE_TIMEOUT_SECS: i64 = 300; // 5 minutes

/// Number of status columns in Kanban view
pub const NUM_COLUMNS: usize = 4;

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
    pub selected_agents: std::collections::HashSet<String>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            agents: HashMap::new(),
            events: VecDeque::new(),
            selected_column: 0,
            selected_card: 0,
            status_counts: [0; NUM_COLUMNS],
            selected_agents: std::collections::HashSet::new(),
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
    pub fn process_event(&mut self, event: HookEvent) {
        let pane_id = event.pane_id.clone();
        let project = event.project.clone();
        let is_new_agent = !self.agents.contains_key(&pane_id);

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

        // Update or create agent
        let agent = self
            .agents
            .entry(pane_id.clone())
            .or_insert_with(|| Agent::new(pane_id.clone(), project));

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
}

fn current_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
