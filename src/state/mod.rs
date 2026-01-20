mod agent;
mod event_processing;
pub mod loop_handling;

pub use agent::{Agent, AgentRole, AttentionType, LoopMode, Status, Subagent};
pub use event_processing::infer_role_from_description;

use crate::event::HookEvent;
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
    /// Path to .rehoboam/ directory for Rehoboam loops (fresh sessions)
    pub loop_dir: Option<std::path::PathBuf>,
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
pub fn status_to_column(status: &Status) -> usize {
    match status {
        Status::Attention(_) => 0,
        Status::Working => 1,
        Status::Compacting => 2,
    }
}


impl AppState {
    pub fn new() -> Self {
        Self::default()
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

                if elapsed > WAITING_TIMEOUT_SECS
                    && agent.current_tool.is_none()
                    && !agent.in_response
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
    /// If `loop_dir` is Some, the agent will use proper Rehoboam mode (fresh sessions).
    /// Judge always runs when loop mode is active (no toggle needed).
    pub fn register_loop_config(
        &mut self,
        pane_id: &str,
        max_iterations: u32,
        stop_word: &str,
        loop_dir: Option<std::path::PathBuf>,
    ) {
        self.pending_loop_configs.insert(
            pane_id.to_string(),
            LoopConfig {
                max_iterations,
                stop_word: stop_word.to_string(),
                loop_dir: loop_dir.clone(),
            },
        );
        tracing::info!(
            pane_id = %pane_id,
            max = max_iterations,
            stop_word = %stop_word,
            loop_dir = ?loop_dir,
            "Registered pending loop config (Judge is automatic)"
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

// NOTE: spawn_fresh_rehoboam_session() is defined in loop_handling.rs

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
            context_window: None,
            agent_type: None,
            permission_mode: None,
            cwd: None,
            transcript_path: None,
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

    // NOTE: is_stalled tests moved to loop_handling.rs

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
                super::infer_role_from_description(desc),
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
}
