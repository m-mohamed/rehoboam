//! Agent state management and event processing
//!
//! Core state machine for tracking Claude Code agents:
//! - Status tracking (Attention, Working, Compacting)
//! - Session lifecycle management
//! - Status count caching for efficient UI rendering

mod agent;
mod event_processing;
mod task_discovery;
mod team_discovery;

pub use agent::{Agent, AgentRole, AttentionType, Status, Subagent, TaskInfo, TaskStatus};
pub use task_discovery::{FsTaskList, TaskDiscovery};
pub use team_discovery::TeamDiscovery;

use crate::event::HookEvent;
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{SystemTime, UNIX_EPOCH};

/// Number of status columns in Kanban view (Attention, Working, Compacting)
pub const NUM_COLUMNS: usize = 3;

/// A task with contextual metadata for display in the task board
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields used by UI renderer and tests
pub struct TaskWithContext {
    pub task_id: String,
    pub subject: String,
    pub status: TaskStatus,
    pub blocked_by: Vec<String>,
    pub blocks: Vec<String>,
    pub owner_name: String,
    pub team_name: Option<String>,
    pub description: String,
    pub active_form: Option<String>,
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
    /// Set of sprite agent IDs (for quick lookup)
    pub sprite_agent_ids: HashSet<String>,
    /// Set of currently connected sprite IDs
    pub connected_sprites: HashSet<String>,
    /// Health warning message (hooks.log size issue)
    pub health_warning: Option<String>,
    /// Configurable timeout: Working → Attention(Waiting) transition (seconds)
    pub idle_timeout_secs: i64,
    /// Configurable timeout: removing stale sessions (seconds)
    pub stale_timeout_secs: i64,
    /// Session ID → team name mapping for cross-event correlation
    pub session_to_team: HashMap<String, String>,
    /// Last filesystem team scan timestamp (throttled to every 30s)
    pub last_team_scan: i64,
    /// Filesystem task lists from ~/.claude/tasks/ (ground truth)
    pub fs_task_lists: HashMap<String, FsTaskList>,
    /// Last filesystem task scan timestamp (throttled to every 10s)
    pub last_task_scan: i64,
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
            sprite_agent_ids: HashSet::new(),
            connected_sprites: HashSet::new(),
            health_warning: None,
            idle_timeout_secs: 60,
            stale_timeout_secs: 300,
            session_to_team: HashMap::new(),
            last_team_scan: 0,
            fs_task_lists: HashMap::new(),
            last_task_scan: 0,
        }
    }
}

/// Map status to column index
pub fn status_to_column(status: &Status) -> usize {
    match status {
        Status::Attention(_) => 0,
        Status::Working => 1,
        Status::Compacting => 2,
    }
}

impl AppState {
    #[allow(dead_code)] // Used in tests; production uses with_timeouts()
    pub fn new() -> Self {
        Self::default()
    }

    /// Create with config-driven timeouts
    pub fn with_timeouts(idle_timeout_secs: i64, stale_timeout_secs: i64) -> Self {
        Self {
            idle_timeout_secs,
            stale_timeout_secs,
            ..Self::default()
        }
    }

    // NOTE: process_event() is defined in event_processing.rs

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
            self.selected_agents.remove(&pane_id);
            self.sprite_agent_ids.remove(&pane_id);
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
                self.selected_agents.remove(&pane_id);
                self.sprite_agent_ids.remove(&pane_id);
            }
        }
    }

    /// Periodic tick for timeout-based state transitions
    ///
    /// Handles:
    /// - Working → Attention(Waiting) after idle_timeout_secs of no events
    /// - Remove stale sessions after stale_timeout_secs of no events
    pub fn tick(&mut self) {
        let now = current_timestamp();
        let mut to_remove: Vec<String> = Vec::new();
        let mut waiting_transitions: Vec<String> = Vec::new();

        let idle_timeout = self.idle_timeout_secs;
        let stale_timeout = self.stale_timeout_secs;

        for (pane_id, agent) in &self.agents {
            let elapsed = now - agent.last_update;

            // Remove stale sessions
            if elapsed > stale_timeout {
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
                        timeout_threshold = idle_timeout,
                        "Timeout check"
                    );
                }

                if elapsed > idle_timeout && agent.current_tool.is_none() && !agent.in_response {
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
                    stale_timeout_secs = stale_timeout,
                    "Removed stale session"
                );
            }
            self.agents.remove(&pane_id);
            self.selected_agents.remove(&pane_id);
            self.sprite_agent_ids.remove(&pane_id);
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
        columns[0].sort_by(|a, b| match (&a.status, &b.status) {
            (Status::Attention(a_type), Status::Attention(b_type)) => a_type
                .priority()
                .cmp(&b_type.priority())
                .then_with(|| a.project.cmp(&b.project)),
            _ => a.project.cmp(&b.project),
        });
        // Sort other columns by project name for consistent ordering
        for col in &mut columns[1..] {
            col.sort_by(|a, b| a.project.cmp(&b.project));
        }
        columns
    }

    /// Get agents grouped by team name
    ///
    /// Returns a vector of (team_name, agents) tuples.
    /// Agents within each team are sorted: leads first, then by status priority.
    /// "Independent" group (agents with no team) is always last.
    pub fn agents_by_team(&self) -> Vec<(String, Vec<&Agent>)> {
        let mut teams: HashMap<String, Vec<&Agent>> = HashMap::new();
        for agent in self.agents.values() {
            let team_key = agent
                .team_name
                .clone()
                .unwrap_or_else(|| "Independent".to_string());
            teams.entry(team_key).or_default().push(agent);
        }
        // Sort: leads first within team, then by status priority
        for agents in teams.values_mut() {
            agents.sort_by(|a, b| {
                let a_lead = a.team_agent_type.as_deref() == Some("lead");
                let b_lead = b.team_agent_type.as_deref() == Some("lead");
                b_lead
                    .cmp(&a_lead)
                    .then_with(|| a.status.priority().cmp(&b.status.priority()))
            });
        }
        // "Independent" always last, otherwise alphabetical
        let mut result: Vec<_> = teams.into_iter().collect();
        result.sort_by(|a, b| {
            (a.0 == "Independent")
                .cmp(&(b.0 == "Independent"))
                .then_with(|| a.0.cmp(&b.0))
        });
        result
    }

    /// Move to next agent in flat order (across all columns)
    ///
    /// Navigates across all status columns in flat order.
    /// Order: Attention agents, then Working agents, then Compacting agents.
    pub fn next_agent_flat(&mut self) {
        let columns = self.agents_by_column();

        // Build flat list of (column_index, card_index) pairs
        let flat: Vec<(usize, usize)> = columns
            .iter()
            .enumerate()
            .flat_map(|(col_idx, agents)| {
                (0..agents.len()).map(move |card_idx| (col_idx, card_idx))
            })
            .collect();

        if flat.is_empty() {
            return;
        }

        // Find current position in flat list
        let current = (self.selected_column, self.selected_card);
        let current_idx = flat.iter().position(|&pos| pos == current).unwrap_or(0);

        // Move to next
        let next_idx = (current_idx + 1) % flat.len();
        let (col, card) = flat[next_idx];
        self.selected_column = col;
        self.selected_card = card;
    }

    /// Move to previous agent in flat order (across all columns)
    ///
    /// Navigates across all status columns in flat order.
    /// Order: Attention agents, then Working agents, then Compacting agents.
    pub fn previous_agent_flat(&mut self) {
        let columns = self.agents_by_column();

        // Build flat list of (column_index, card_index) pairs
        let flat: Vec<(usize, usize)> = columns
            .iter()
            .enumerate()
            .flat_map(|(col_idx, agents)| {
                (0..agents.len()).map(move |card_idx| (col_idx, card_idx))
            })
            .collect();

        if flat.is_empty() {
            return;
        }

        // Find current position in flat list
        let current = (self.selected_column, self.selected_card);
        let current_idx = flat.iter().position(|&pos| pos == current).unwrap_or(0);

        // Move to previous (wrap around)
        let prev_idx = if current_idx == 0 {
            flat.len() - 1
        } else {
            current_idx - 1
        };
        let (col, card) = flat[prev_idx];
        self.selected_column = col;
        self.selected_card = card;
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

    /// Get the pane_id of the currently selected agent
    #[allow(dead_code)] // API consistency: convenience method
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

    /// Map a session ID to a team name for cross-event correlation
    pub fn map_session_to_team(&mut self, session_id: String, team_name: String) {
        self.session_to_team.insert(session_id, team_name);
    }

    /// Look up team name for a session ID
    #[allow(dead_code)] // Public API used in tests; production uses session_to_team directly
    pub fn get_team_for_session(&self, session_id: &str) -> Option<&str> {
        self.session_to_team.get(session_id).map(|s| s.as_str())
    }

    /// Periodically scan ~/.claude/teams/ and enrich agents with discovered team membership
    ///
    /// Throttled to every 30s. Matches agents to teams by team_agent_name or team_agent_id
    /// from `~/.claude/teams/*/config.json` members.
    pub fn refresh_team_metadata(&mut self) {
        let now = current_timestamp();
        if self.last_team_scan != 0 && now - self.last_team_scan < 30 {
            return;
        }
        self.last_team_scan = now;

        match TeamDiscovery::scan_teams() {
            Ok(teams) => {
                let mut enriched = 0u32;
                for config in teams.values() {
                    for member in &config.members {
                        for agent in self.agents.values_mut() {
                            if agent.team_name.is_some() {
                                continue;
                            }
                            // Match by team_agent_name → member.name
                            let name_match = agent.team_agent_name.as_deref() == Some(&member.name);
                            // Match by team_agent_id → member.agent_id
                            let id_match = !member.agent_id.is_empty()
                                && agent.team_agent_id.as_deref() == Some(&member.agent_id);
                            // Match by tmux pane ID
                            let pane_match = member
                                .tmux_pane_id
                                .as_deref()
                                .is_some_and(|pane| pane == agent.pane_id);

                            if name_match || id_match || pane_match {
                                agent.team_name = Some(config.team_name.clone());
                                agent.team_agent_type = Some(member.agent_type.clone());
                                if agent.team_agent_name.is_none() {
                                    agent.team_agent_name = Some(member.name.clone());
                                }
                                // Detect team lead
                                if config.lead_agent_id.as_deref() == Some(&member.agent_id) {
                                    agent.team_agent_type = Some("lead".to_string());
                                }
                                enriched += 1;
                            }
                        }
                    }
                }
                if enriched > 0 {
                    tracing::info!(
                        enriched = enriched,
                        teams_found = teams.len(),
                        "Enriched agents from filesystem team discovery"
                    );
                }

                // Task list correlation: link agents to their team's task list
                for team_name in teams.keys() {
                    if self.fs_task_lists.contains_key(team_name) {
                        for agent in self.agents.values_mut() {
                            if agent.team_name.as_deref() == Some(team_name)
                                && agent.task_list_id.is_none()
                            {
                                agent.task_list_id = Some(team_name.clone());
                            }
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "Failed to scan ~/.claude/teams/ for team discovery"
                );
            }
        }
    }

    /// Periodically scan ~/.claude/tasks/ and update filesystem task lists
    ///
    /// Throttled to every 10s. Provides ground truth for task state.
    pub fn refresh_task_data(&mut self) {
        let now = current_timestamp();
        if self.last_task_scan != 0 && now - self.last_task_scan < 10 {
            return;
        }
        self.last_task_scan = now;
        match TaskDiscovery::scan_tasks() {
            Ok(lists) => {
                self.fs_task_lists = lists;
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to scan ~/.claude/tasks/");
            }
        }
    }

    /// Get tasks grouped by team name
    ///
    /// Returns `Vec<(String, [Vec<TaskWithContext>; 3])>` — grouped by team,
    /// each with \[pending, in_progress, completed\] arrays.
    ///
    /// Merges filesystem tasks (ground truth) with hook-captured tasks from
    /// agent.tasks. Deduplicates by task_id (filesystem wins for status).
    /// Includes phantom agent current_task_subject as in_progress tasks.
    /// Sorts: blocked tasks to end, then alphabetical by subject.
    pub fn tasks_by_team(&self) -> Vec<(String, [Vec<TaskWithContext>; 3])> {
        let mut team_tasks: HashMap<String, HashMap<String, TaskWithContext>> = HashMap::new();

        // Build a map of team names from agents_by_team() for correlating fs list IDs
        let known_teams: HashMap<String, String> = self
            .agents_by_team()
            .into_iter()
            .filter(|(name, _)| name != "Independent")
            .map(|(name, _)| (name.clone(), name))
            .collect();

        // Step 1: Filesystem tasks (ground truth)
        for (list_id, fs_list) in &self.fs_task_lists {
            // Determine team name: use known team name if it matches, otherwise use list_id
            let team_name = if known_teams.contains_key(list_id) {
                list_id.clone()
            } else {
                // Try to correlate UUID-based list via agent.task_list_id
                let mut found_team = None;
                for agent in self.agents.values() {
                    if agent.task_list_id.as_deref() == Some(list_id) {
                        if let Some(tn) = &agent.team_name {
                            found_team = Some(tn.clone());
                            break;
                        }
                    }
                }
                found_team.unwrap_or_else(|| list_id.clone())
            };

            let tasks = team_tasks.entry(team_name.clone()).or_default();
            for fs_task in &fs_list.tasks {
                tasks.insert(
                    fs_task.id.clone(),
                    TaskWithContext {
                        task_id: fs_task.id.clone(),
                        subject: fs_task.subject.clone(),
                        status: TaskStatus::from_str(&fs_task.status),
                        blocked_by: fs_task.blocked_by.clone(),
                        blocks: fs_task.blocks.clone(),
                        owner_name: String::new(),
                        team_name: Some(team_name.clone()),
                        description: fs_task.description.clone(),
                        active_form: fs_task.active_form.clone(),
                    },
                );
            }
        }

        // Step 2: Merge hook-captured tasks from agents (deduplicate, fs wins)
        for agent in self.agents.values() {
            let team_name = agent
                .team_name
                .clone()
                .unwrap_or_else(|| "Independent".to_string());
            let tasks = team_tasks.entry(team_name.clone()).or_default();

            for (task_id, task_info) in &agent.tasks {
                if !tasks.contains_key(task_id) {
                    tasks.insert(
                        task_id.clone(),
                        TaskWithContext {
                            task_id: task_id.clone(),
                            subject: task_info.subject.clone(),
                            status: task_info.status,
                            blocked_by: task_info.blocked_by.clone(),
                            blocks: task_info.blocks.clone(),
                            owner_name: agent
                                .team_agent_name
                                .clone()
                                .unwrap_or_default(),
                            team_name: Some(team_name.clone()),
                            description: String::new(),
                            active_form: None,
                        },
                    );
                }
            }
        }

        // Step 3: Include phantom agent current_task_subject as in_progress tasks
        for agent in self.agents.values() {
            if agent.pane_id.starts_with("team:") {
                if let Some(subject) = &agent.current_task_subject {
                    let team_name = agent
                        .team_name
                        .clone()
                        .unwrap_or_else(|| "Independent".to_string());
                    let tasks = team_tasks.entry(team_name.clone()).or_default();

                    // Use a synthetic task ID to avoid collisions
                    let synthetic_id = format!("phantom:{}", agent.pane_id);
                    if !tasks.values().any(|t| t.subject == *subject) {
                        tasks.insert(
                            synthetic_id,
                            TaskWithContext {
                                task_id: String::new(),
                                subject: subject.clone(),
                                status: TaskStatus::InProgress,
                                blocked_by: Vec::new(),
                                blocks: Vec::new(),
                                owner_name: agent
                                    .team_agent_name
                                    .clone()
                                    .unwrap_or_default(),
                                team_name: Some(team_name.clone()),
                                description: String::new(),
                                active_form: None,
                            },
                        );
                    }
                }
            }
        }

        // Step 4: Group by team, sort within each status column
        let mut result: Vec<(String, [Vec<TaskWithContext>; 3])> = team_tasks
            .into_iter()
            .map(|(team_name, tasks_map)| {
                let mut columns: [Vec<TaskWithContext>; 3] = Default::default();
                for task in tasks_map.into_values() {
                    let col = match task.status {
                        TaskStatus::Pending => 0,
                        TaskStatus::InProgress => 1,
                        TaskStatus::Completed => 2,
                    };
                    columns[col].push(task);
                }
                // Sort each column: blocked tasks to end, then alphabetical
                for col in &mut columns {
                    col.sort_by(|a, b| {
                        let a_blocked = !a.blocked_by.is_empty();
                        let b_blocked = !b.blocked_by.is_empty();
                        a_blocked
                            .cmp(&b_blocked)
                            .then_with(|| a.subject.cmp(&b.subject))
                    });
                }
                (team_name, columns)
            })
            .collect();

        // Sort teams: "Independent" last, otherwise alphabetical
        result.sort_by(|a, b| {
            (a.0 == "Independent")
                .cmp(&(b.0 == "Independent"))
                .then_with(|| a.0.cmp(&b.0))
        });

        result
    }

    /// Set the working directory for an agent (for git operations)
    pub fn set_agent_working_dir(&mut self, pane_id: &str, working_dir: std::path::PathBuf) {
        if let Some(agent) = self.agents.get_mut(pane_id) {
            agent.working_dir = Some(working_dir);
        }
    }

    /// Set the task list ID for an agent (Claude Code Tasks API)
    pub fn set_agent_task_list_id(&mut self, pane_id: &str, task_list_id: String) {
        if let Some(agent) = self.agents.get_mut(pane_id) {
            agent.task_list_id = Some(task_list_id);
            tracing::info!(
                pane_id = %pane_id,
                task_list_id = ?agent.task_list_id,
                "Set agent task list ID"
            );
        }
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
    use super::event_processing::infer_role_from_description;
    use super::task_discovery::FsTask;
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
            notification_type: None,
            notification_title: None,
            error: None,
            is_interrupt: None,
            prompt: None,
            subagent_id: None,
            description: None,
            subagent_duration_ms: None,
            source: crate::event::EventSource::Local,
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
    fn test_subagent_stop_does_not_override_attention() {
        let mut state = AppState::new();

        // First create the agent with a working event
        let _ = state.process_event(make_event("SessionStart", "working", "%0", "test"));

        // Start AskUserQuestion - should transition to Attention(Input)
        let mut event = make_event("PreToolUse", "working", "%0", "test");
        event.tool_name = Some("AskUserQuestion".to_string());
        let _ = state.process_event(event);

        // Verify in Attention(Input)
        assert!(
            matches!(
                state.agents.get("%0").unwrap().status,
                Status::Attention(AttentionType::Input)
            ),
            "Expected Attention(Input) after AskUserQuestion PreToolUse"
        );

        // SubagentStop arrives with status "working" - should NOT change status
        let mut event = make_event("SubagentStop", "working", "%0", "test");
        event.subagent_id = Some("sub-123".to_string());
        let _ = state.process_event(event);

        // Should STILL be Attention(Input)
        assert!(
            matches!(
                state.agents.get("%0").unwrap().status,
                Status::Attention(AttentionType::Input)
            ),
            "SubagentStop should not override Attention(Input)"
        );
    }

    #[test]
    fn test_subagent_start_does_not_override_attention() {
        let mut state = AppState::new();

        // First create the agent with a working event
        let _ = state.process_event(make_event("SessionStart", "working", "%0", "test"));

        // Start AskUserQuestion - should transition to Attention(Input)
        let mut event = make_event("PreToolUse", "working", "%0", "test");
        event.tool_name = Some("AskUserQuestion".to_string());
        let _ = state.process_event(event);

        // SubagentStart arrives with status "working" - should NOT change status
        let mut event = make_event("SubagentStart", "working", "%0", "test");
        event.subagent_id = Some("sub-456".to_string());
        event.description = Some("Exploring codebase".to_string());
        let _ = state.process_event(event);

        // Should STILL be Attention(Input)
        assert!(
            matches!(
                state.agents.get("%0").unwrap().status,
                Status::Attention(AttentionType::Input)
            ),
            "SubagentStart should not override Attention(Input)"
        );
    }

    #[test]
    fn test_agents_by_team_grouping() {
        let mut state = AppState::new();

        // Create a team lead
        let mut event = make_event("SessionStart", "working", "%lead", "auth-project");
        event.team_name = Some("refactor-auth".to_string());
        event.team_agent_type = Some("lead".to_string());
        event.team_agent_name = Some("lead-agent".to_string());
        let _ = state.process_event(event);

        // Create a team worker
        let mut event = make_event("SessionStart", "working", "%worker", "auth-project");
        event.team_name = Some("refactor-auth".to_string());
        event.team_agent_type = Some("worker".to_string());
        event.team_agent_name = Some("test-writer".to_string());
        let _ = state.process_event(event);

        // Create an independent agent
        let _ = state.process_event(make_event("SessionStart", "working", "%solo", "other"));

        let teams = state.agents_by_team();

        // Should have 2 groups
        assert_eq!(teams.len(), 2, "should have 2 groups (team + independent)");

        // First group: "refactor-auth" (alphabetically before "Independent" is forced last)
        assert_eq!(teams[0].0, "refactor-auth");
        assert_eq!(teams[0].1.len(), 2);
        // Lead should be first
        assert_eq!(
            teams[0].1[0].team_agent_type.as_deref(),
            Some("lead"),
            "lead should be first in team"
        );

        // Second group: "Independent"
        assert_eq!(teams[1].0, "Independent");
        assert_eq!(teams[1].1.len(), 1);
        assert_eq!(teams[1].1[0].pane_id, "%solo");
    }

    #[test]
    fn test_agents_by_team_empty() {
        let state = AppState::new();
        let teams = state.agents_by_team();
        assert!(teams.is_empty(), "empty state should return empty vec");
    }

    // v1.3: Tests for parent-child tracking and role inference from description

    #[test]
    fn test_subagent_role_from_description() {
        // Table-driven test for role inference from description
        let cases = vec![
            // Planner keywords
            ("Explore codebase structure", AgentRole::Planner),
            ("Research API patterns", AgentRole::Planner),
            ("Search for error handlers", AgentRole::Planner),
            // Worker keywords
            ("Implement the login feature", AgentRole::Worker),
            ("Fix the bug in auth", AgentRole::Worker),
            ("Write tests for API", AgentRole::Worker),
            // Reviewer keywords (avoid Worker keywords like "change", "update")
            ("Review the pull request", AgentRole::Reviewer),
            ("Test the results", AgentRole::Reviewer),
            ("Verify all passes", AgentRole::Reviewer),
        ];
        for (desc, expected) in cases {
            assert_eq!(
                infer_role_from_description(desc),
                expected,
                "description: {}",
                desc
            );
        }
    }

    #[test]
    fn test_subagent_parent_tracking() {
        let mut state = AppState::new();

        // Create parent agent
        let _ = state.process_event(make_event("SessionStart", "working", "%0", "test"));

        // Spawn subagent
        let mut event = make_event("SubagentStart", "working", "%0", "test");
        event.subagent_id = Some("sub-123".to_string());
        event.description = Some("Explore the codebase".to_string());
        let _ = state.process_event(event);

        // Verify subagent was tracked with parent info
        let agent = state.agents.get("%0").unwrap();
        assert_eq!(agent.subagents.len(), 1);
        let subagent = &agent.subagents[0];
        assert_eq!(subagent.parent_pane_id, "%0");
        assert_eq!(subagent.depth, 0);
        assert_eq!(subagent.role, AgentRole::Planner); // "Explore" -> Planner
    }

    // =========================================================================
    // v0.9.16 feature tests
    // =========================================================================

    #[test]
    fn test_notification_permission_prompt_becomes_attention_permission() {
        let mut state = AppState::new();
        let _ = state.process_event(make_event("SessionStart", "working", "%0", "test"));

        // Notification with notification_type="permission_prompt" should become Permission
        let mut event = make_event("Notification", "attention", "%0", "test");
        event.attention_type = Some("notification".to_string());
        event.notification_type = Some("permission_prompt".to_string());
        let _ = state.process_event(event);

        let agent = state.agents.get("%0").unwrap();
        assert!(
            matches!(agent.status, Status::Attention(AttentionType::Permission)),
            "permission_prompt notification should become Attention(Permission), got {:?}",
            agent.status
        );
        assert_eq!(
            agent.last_notification_type.as_deref(),
            Some("permission_prompt")
        );
    }

    #[test]
    fn test_notification_idle_prompt_becomes_attention_input() {
        let mut state = AppState::new();
        let _ = state.process_event(make_event("SessionStart", "working", "%0", "test"));

        // Notification with notification_type="idle_prompt" should become Input
        let mut event = make_event("Notification", "attention", "%0", "test");
        event.attention_type = Some("notification".to_string());
        event.notification_type = Some("idle_prompt".to_string());
        let _ = state.process_event(event);

        let agent = state.agents.get("%0").unwrap();
        assert!(
            matches!(agent.status, Status::Attention(AttentionType::Input)),
            "idle_prompt notification should become Attention(Input), got {:?}",
            agent.status
        );
    }

    #[test]
    fn test_post_tool_use_failure_error_forwarding() {
        let mut state = AppState::new();
        let _ = state.process_event(make_event("SessionStart", "working", "%0", "test"));

        // Start a tool
        let mut pre = make_event("PreToolUse", "working", "%0", "test");
        pre.tool_name = Some("Bash".to_string());
        pre.tool_use_id = Some("tu-1".to_string());
        let _ = state.process_event(pre);

        // Fail the tool with error and interrupt
        let mut fail = make_event("PostToolUseFailure", "working", "%0", "test");
        fail.tool_use_id = Some("tu-1".to_string());
        fail.error = Some("Command timed out".to_string());
        fail.is_interrupt = Some(true);
        let _ = state.process_event(fail);

        let agent = state.agents.get("%0").unwrap();
        assert!(agent.last_tool_failed);
        assert_eq!(agent.failed_tool_name.as_deref(), Some("Bash"));
        assert_eq!(
            agent.failed_tool_error.as_deref(),
            Some("Command timed out")
        );
        assert!(agent.failed_tool_interrupt);
    }

    #[test]
    fn test_post_tool_use_failure_error_without_interrupt() {
        let mut state = AppState::new();
        let _ = state.process_event(make_event("SessionStart", "working", "%0", "test"));

        // Start a tool
        let mut pre = make_event("PreToolUse", "working", "%0", "test");
        pre.tool_name = Some("Write".to_string());
        let _ = state.process_event(pre);

        // Fail the tool with error only
        let mut fail = make_event("PostToolUseFailure", "working", "%0", "test");
        fail.error = Some("Permission denied".to_string());
        fail.is_interrupt = Some(false);
        let _ = state.process_event(fail);

        let agent = state.agents.get("%0").unwrap();
        assert!(agent.last_tool_failed);
        assert_eq!(
            agent.failed_tool_error.as_deref(),
            Some("Permission denied")
        );
        assert!(!agent.failed_tool_interrupt);
    }

    #[test]
    fn test_session_start_with_session_source() {
        let mut state = AppState::new();

        let mut event = make_event("SessionStart", "working", "%0", "test");
        event.session_source = Some("resume".to_string());
        let _ = state.process_event(event);

        let agent = state.agents.get("%0").unwrap();
        assert_eq!(agent.session_source.as_deref(), Some("resume"));
    }

    #[test]
    fn test_stop_with_stop_hook_active() {
        let mut state = AppState::new();
        let _ = state.process_event(make_event("SessionStart", "working", "%0", "test"));

        let mut event = make_event("Stop", "idle", "%0", "test");
        event.stop_hook_active = Some(true);
        let _ = state.process_event(event);

        let agent = state.agents.get("%0").unwrap();
        assert!(agent.stop_hook_active);
    }

    #[test]
    fn test_stop_hook_active_cleared_on_normal_stop() {
        let mut state = AppState::new();
        let _ = state.process_event(make_event("SessionStart", "working", "%0", "test"));

        // First stop with hook active
        let mut event = make_event("Stop", "idle", "%0", "test");
        event.stop_hook_active = Some(true);
        let _ = state.process_event(event);
        assert!(state.agents.get("%0").unwrap().stop_hook_active);

        // Resume working
        let _ = state.process_event(make_event("UserPromptSubmit", "working", "%0", "test"));

        // Stop without hook active
        let mut event = make_event("Stop", "idle", "%0", "test");
        event.stop_hook_active = Some(false);
        let _ = state.process_event(event);
        assert!(!state.agents.get("%0").unwrap().stop_hook_active);
    }

    #[test]
    fn test_subagent_start_with_agent_type() {
        let mut state = AppState::new();
        let _ = state.process_event(make_event("SessionStart", "working", "%0", "test"));

        // SubagentStart with agent_type
        let mut event = make_event("SubagentStart", "working", "%0", "test");
        event.subagent_id = Some("sub-789".to_string());
        event.description = Some("Quick search".to_string());
        event.agent_type = Some("Explore".to_string());
        let _ = state.process_event(event);

        let agent = state.agents.get("%0").unwrap();
        assert_eq!(agent.subagents.len(), 1);
        assert_eq!(agent.subagents[0].subagent_type.as_deref(), Some("Explore"));
    }

    #[test]
    fn test_model_tracking_from_session_start() {
        let mut state = AppState::new();

        let mut event = make_event("SessionStart", "working", "%0", "test");
        event.model = Some("claude-opus-4-6".to_string());
        let _ = state.process_event(event);

        let agent = state.agents.get("%0").unwrap();
        assert_eq!(agent.model.as_deref(), Some("claude-opus-4-6"));
    }

    #[test]
    fn test_context_window_tracking() {
        let mut state = AppState::new();
        let _ = state.process_event(make_event("SessionStart", "working", "%0", "test"));

        let mut event = make_event("PreToolUse", "working", "%0", "test");
        event.tool_name = Some("Read".to_string());
        event.context_window = Some(crate::event::ContextWindow {
            used_percentage: Some(73.5),
            remaining_percentage: Some(26.5),
            total_tokens: Some(150000),
        });
        let _ = state.process_event(event);

        let agent = state.agents.get("%0").unwrap();
        assert_eq!(agent.context_usage_percent, Some(73.5));
        assert_eq!(agent.context_remaining_percent, Some(26.5));
        assert_eq!(agent.context_total_tokens, Some(150000));
    }

    #[test]
    fn test_effort_level_captured_from_event() {
        let mut state = AppState::new();
        let mut event = make_event("SessionStart", "working", "%0", "test");
        event.effort_level = Some("high".to_string());
        let _ = state.process_event(event);

        let agent = state.agents.get("%0").unwrap();
        assert_eq!(agent.effort_level.as_deref(), Some("high"));
    }

    #[test]
    fn test_effort_level_absent_graceful_degradation() {
        let mut state = AppState::new();
        let event = make_event("SessionStart", "working", "%0", "test");
        let _ = state.process_event(event);

        let agent = state.agents.get("%0").unwrap();
        assert!(
            agent.effort_level.is_none(),
            "effort_level should be None when not set"
        );
    }

    #[test]
    fn test_session_to_team_correlation() {
        let mut state = AppState::new();

        // Map session to team
        state.map_session_to_team("session-abc".to_string(), "refactor-auth".to_string());

        // Verify lookup works
        assert_eq!(
            state.get_team_for_session("session-abc"),
            Some("refactor-auth")
        );

        // Unknown session returns None
        assert_eq!(state.get_team_for_session("session-unknown"), None);
    }

    #[test]
    fn test_phantom_agent_creation() {
        let mut state = AppState::new();

        // Simulate TeammateIdle event creating a phantom agent
        let mut event = make_event("TeammateIdle", "working", "team:my-team:researcher", "test");
        event.team_name = Some("my-team".to_string());
        event.teammate_name = Some("researcher".to_string());
        let _ = state.process_event(event);

        // Phantom agent should exist
        assert!(state.agents.contains_key("team:my-team:researcher"));
        let agent = state.agents.get("team:my-team:researcher").unwrap();
        assert_eq!(agent.team_name.as_deref(), Some("my-team"));
        assert_eq!(agent.team_agent_name.as_deref(), Some("researcher"));
    }

    #[test]
    fn test_agents_by_team_with_phantoms() {
        let mut state = AppState::new();

        // Create a real team lead
        let mut event = make_event("SessionStart", "working", "%lead", "auth-project");
        event.team_name = Some("refactor-auth".to_string());
        event.team_agent_type = Some("lead".to_string());
        event.team_agent_name = Some("lead-agent".to_string());
        let _ = state.process_event(event);

        // Create a phantom teammate via TeammateIdle
        let mut event = make_event(
            "TeammateIdle",
            "working",
            "team:refactor-auth:worker-1",
            "auth-project",
        );
        event.team_name = Some("refactor-auth".to_string());
        event.teammate_name = Some("worker-1".to_string());
        let _ = state.process_event(event);

        let teams = state.agents_by_team();

        // Should have 1 team group with both agents
        assert_eq!(teams.len(), 1);
        assert_eq!(teams[0].0, "refactor-auth");
        assert_eq!(teams[0].1.len(), 2);
    }

    #[test]
    fn test_tool_failure_cleared_on_next_tool() {
        let mut state = AppState::new();
        let _ = state.process_event(make_event("SessionStart", "working", "%0", "test"));

        // Fail a tool
        let mut pre = make_event("PreToolUse", "working", "%0", "test");
        pre.tool_name = Some("Bash".to_string());
        let _ = state.process_event(pre);

        let mut fail = make_event("PostToolUseFailure", "working", "%0", "test");
        fail.error = Some("error".to_string());
        let _ = state.process_event(fail);

        assert!(state.agents.get("%0").unwrap().last_tool_failed);

        // New PreToolUse should clear failure state
        let mut pre2 = make_event("PreToolUse", "working", "%0", "test");
        pre2.tool_name = Some("Read".to_string());
        let _ = state.process_event(pre2);

        let agent = state.agents.get("%0").unwrap();
        assert!(!agent.last_tool_failed);
        assert!(agent.failed_tool_name.is_none());
        assert!(agent.failed_tool_error.is_none());
        assert!(!agent.failed_tool_interrupt);
    }

    // =========================================================================
    // Tasks by team tests
    // =========================================================================

    #[test]
    fn test_tasks_by_team_empty() {
        let state = AppState::new();
        let teams = state.tasks_by_team();
        assert!(teams.is_empty(), "empty state should return empty vec");
    }

    #[test]
    fn test_tasks_by_team_from_filesystem() {
        let mut state = AppState::new();

        // Directly set fs_task_lists (simulating filesystem scan)
        let mut lists = HashMap::new();
        lists.insert(
            "my-team".to_string(),
            FsTaskList {
                list_id: "my-team".to_string(),
                tasks: vec![
                    FsTask {
                        id: "1".to_string(),
                        subject: "Build API".to_string(),
                        description: "Build the REST API".to_string(),
                        active_form: Some("Building API".to_string()),
                        status: "pending".to_string(),
                        blocks: vec!["2".to_string()],
                        blocked_by: Vec::new(),
                    },
                    FsTask {
                        id: "2".to_string(),
                        subject: "Write tests".to_string(),
                        description: "Write integration tests".to_string(),
                        active_form: None,
                        status: "in_progress".to_string(),
                        blocks: Vec::new(),
                        blocked_by: vec!["1".to_string()],
                    },
                    FsTask {
                        id: "3".to_string(),
                        subject: "Setup CI".to_string(),
                        description: "Configure CI pipeline".to_string(),
                        active_form: None,
                        status: "completed".to_string(),
                        blocks: Vec::new(),
                        blocked_by: Vec::new(),
                    },
                ],
            },
        );
        state.fs_task_lists = lists;

        let teams = state.tasks_by_team();
        assert_eq!(teams.len(), 1);
        assert_eq!(teams[0].0, "my-team");

        let [pending, in_progress, completed] = &teams[0].1;
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].subject, "Build API");
        assert_eq!(in_progress.len(), 1);
        assert_eq!(in_progress[0].subject, "Write tests");
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].subject, "Setup CI");
    }

    #[test]
    fn test_tasks_by_team_merge_dedup() {
        let mut state = AppState::new();

        // Create an agent with hook-captured tasks
        let mut event = make_event("SessionStart", "working", "%0", "test");
        event.team_name = Some("my-team".to_string());
        event.team_agent_name = Some("worker-1".to_string());
        let _ = state.process_event(event);

        // Add hook-captured tasks to the agent
        let agent = state.agents.get_mut("%0").unwrap();
        agent.tasks.insert(
            "1".to_string(),
            TaskInfo {
                id: "1".to_string(),
                subject: "Build API (hook)".to_string(),
                status: TaskStatus::InProgress, // Hook says in_progress
                blocked_by: Vec::new(),
                blocks: Vec::new(),
            },
        );
        agent.tasks.insert(
            "99".to_string(),
            TaskInfo {
                id: "99".to_string(),
                subject: "Hook-only task".to_string(),
                status: TaskStatus::Pending,
                blocked_by: Vec::new(),
                blocks: Vec::new(),
            },
        );

        // Filesystem says task 1 is still pending (filesystem wins)
        let mut lists = HashMap::new();
        lists.insert(
            "my-team".to_string(),
            FsTaskList {
                list_id: "my-team".to_string(),
                tasks: vec![FsTask {
                    id: "1".to_string(),
                    subject: "Build API".to_string(),
                    description: "".to_string(),
                    active_form: None,
                    status: "pending".to_string(),
                    blocks: Vec::new(),
                    blocked_by: Vec::new(),
                }],
            },
        );
        state.fs_task_lists = lists;

        let teams = state.tasks_by_team();
        assert_eq!(teams.len(), 1);
        assert_eq!(teams[0].0, "my-team");

        let [pending, in_progress, _completed] = &teams[0].1;

        // Task "1" should be pending (filesystem wins over hook's in_progress)
        let task_1 = pending.iter().find(|t| t.task_id == "1");
        assert!(task_1.is_some(), "filesystem task should be present");
        assert_eq!(task_1.unwrap().subject, "Build API"); // filesystem subject

        // Hook-only task "99" should also appear in pending (its hook status)
        let task_99 = pending.iter().find(|t| t.task_id == "99");
        assert!(task_99.is_some(), "hook-only task should be included");
        assert_eq!(task_99.unwrap().subject, "Hook-only task");

        assert_eq!(pending.len(), 2, "should have fs task + hook-only task");
        assert_eq!(in_progress.len(), 0, "task 1 should NOT be in_progress (fs wins)");
    }
}
