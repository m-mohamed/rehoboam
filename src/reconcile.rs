//! Tmux-based reconciliation for agent state
//!
//! Supplements unreliable Claude Code hooks by:
//! 1. Detecting permission prompts via tmux pane capture
//! 2. Detecting dead/crashed panes
//! 3. Repairing orphaned state fields that block timeout transitions
//!
//! Runs every `interval_secs` (default 3s) and only checks agents
//! in "uncertain states" (Working with no events for > `uncertain_threshold_secs`).

use std::time::Instant;

use crate::config::{ReconciliationConfig, MAX_EVENTS};
use crate::event::{EventSource, HookEvent};
use crate::state::{status_to_column, AppState, AttentionType, Status};
use crate::tmux::{PromptType, TmuxController};

/// Clear orphaned current_tool after this many seconds
pub const ORPHANED_TOOL_TIMEOUT_SECS: i64 = 120;

/// Clear orphaned in_response after this many seconds
pub const ORPHANED_RESPONSE_TIMEOUT_SECS: i64 = 60;

/// Number of lines to capture from pane for pattern matching
const CAPTURE_LINES: usize = 30;

/// Reconciler state for tmux-based state repair
pub struct Reconciler {
    /// Last reconciliation run
    last_run: Instant,
    /// Whether reconciliation is enabled
    enabled: bool,
    /// Seconds between reconciliation runs
    interval_secs: u64,
    /// Agent is "uncertain" if no events for this many seconds
    uncertain_threshold_secs: i64,
}

impl Default for Reconciler {
    fn default() -> Self {
        Self {
            last_run: Instant::now(),
            enabled: true,
            interval_secs: 3,
            uncertain_threshold_secs: 5,
        }
    }
}

impl Reconciler {
    /// Create a new reconciler from config
    pub fn new(config: &ReconciliationConfig) -> Self {
        Self {
            last_run: Instant::now(),
            enabled: config.enabled,
            interval_secs: config.interval_secs,
            uncertain_threshold_secs: config.uncertain_threshold_secs,
        }
    }

    /// Check if reconciliation should run
    pub fn should_run(&self) -> bool {
        self.enabled && self.last_run.elapsed().as_secs() >= self.interval_secs
    }

    /// Run reconciliation on all agents
    ///
    /// Returns true if any state was modified
    pub fn run(&mut self, state: &mut AppState) -> bool {
        self.last_run = Instant::now();

        if !self.enabled {
            return false;
        }

        let now = current_timestamp();
        let mut modified = false;

        // Collect pane_ids to check (can't iterate mutably while modifying)
        let threshold = self.uncertain_threshold_secs;
        let uncertain_agents: Vec<String> = state
            .agents
            .iter()
            .filter(|(pane_id, agent)| {
                // Only check tmux panes (start with %)
                pane_id.starts_with('%') &&
                // Only check uncertain agents (Working with stale events)
                Self::is_uncertain(agent, now, threshold)
            })
            .map(|(id, _)| id.clone())
            .collect();

        let count = uncertain_agents.len();
        if count > 0 {
            tracing::debug!(count = count, "Reconciler: checking uncertain agents");
        }

        for pane_id in uncertain_agents {
            if self.reconcile_agent(state, &pane_id, now) {
                modified = true;
            }
        }

        // Also repair orphaned fields on phantom agents (non-tmux, team:* prefix)
        let phantom_agents: Vec<String> = state
            .agents
            .iter()
            .filter(|(id, agent)| {
                id.starts_with("team:") && Self::is_uncertain(agent, now, threshold)
            })
            .map(|(id, _)| id.clone())
            .collect();
        for pane_id in phantom_agents {
            if self.repair_orphaned_fields(state, &pane_id, now) {
                modified = true;
            }
        }

        modified
    }

    /// Check if agent is in an uncertain state
    fn is_uncertain(agent: &crate::state::Agent, now: i64, threshold_secs: i64) -> bool {
        let elapsed = now - agent.last_update;

        matches!(agent.status, Status::Working) && elapsed > threshold_secs
    }

    /// Reconcile a single agent's state
    fn reconcile_agent(&self, state: &mut AppState, pane_id: &str, now: i64) -> bool {
        // Phase 1: Check pane health
        match TmuxController::is_pane_alive(pane_id) {
            Ok(false) => {
                // Pane is dead - mark for transition to Waiting
                tracing::warn!(
                    pane_id = %pane_id,
                    "Reconciler: pane is dead, transitioning to Waiting"
                );
                return self.transition_to_waiting(state, pane_id, "Reconciler:PaneDead");
            }
            Err(e) => {
                // Pane doesn't exist or tmux error - log but don't remove
                tracing::debug!(
                    pane_id = %pane_id,
                    error = %e,
                    "Reconciler: pane check failed"
                );
                // Don't remove - could be transient tmux error
                return false;
            }
            Ok(true) => {
                // Pane alive, continue with prompt detection
            }
        }

        // Phase 2: Detect prompts in pane output
        match TmuxController::capture_pane_tail(pane_id, CAPTURE_LINES) {
            Ok(output) => {
                if let Some(prompt_type) = TmuxController::match_prompt_patterns(&output) {
                    return self.handle_prompt_detected(state, pane_id, prompt_type);
                }
            }
            Err(e) => {
                tracing::debug!(
                    pane_id = %pane_id,
                    error = %e,
                    "Reconciler: failed to capture pane output"
                );
            }
        }

        // Phase 3: Repair orphaned state fields
        self.repair_orphaned_fields(state, pane_id, now)
    }

    /// Transition an agent to Attention(Waiting) status
    fn transition_to_waiting(&self, state: &mut AppState, pane_id: &str, event: &str) -> bool {
        let Some(agent) = state.agents.get_mut(pane_id) else {
            return false;
        };

        let old_status = agent.status.clone();
        let new_status = Status::Attention(AttentionType::Waiting);

        if old_status == new_status {
            return false;
        }

        let project = agent.project.clone();

        tracing::info!(
            pane_id = %pane_id,
            project = %project,
            old = ?old_status,
            new = ?new_status,
            "Reconciler: transitioning agent"
        );

        // Update status counts
        let old_col = status_to_column(&old_status);
        let new_col = status_to_column(&new_status);
        state.status_counts[old_col] = state.status_counts[old_col].saturating_sub(1);
        state.status_counts[new_col] += 1;

        agent.status = new_status;
        agent.last_event = event.to_string();
        agent.last_update = current_timestamp();

        // Clear orphaned flags
        agent.in_response = false;
        agent.current_tool = None;

        // Add to event log for debug panel visibility
        push_event(
            state,
            synthetic_event(pane_id, event, "attention", &project),
        );

        true
    }

    /// Handle prompt detected in pane output
    fn handle_prompt_detected(
        &self,
        state: &mut AppState,
        pane_id: &str,
        prompt_type: PromptType,
    ) -> bool {
        let Some(agent) = state.agents.get_mut(pane_id) else {
            return false;
        };

        let old_status = agent.status.clone();
        let new_status = match prompt_type {
            PromptType::Permission => Status::Attention(AttentionType::Permission),
            PromptType::Input => Status::Attention(AttentionType::Input),
        };

        if old_status == new_status {
            return false;
        }

        let project = agent.project.clone();
        let event_name = format!("Reconciler:{prompt_type:?}");

        tracing::info!(
            pane_id = %pane_id,
            project = %project,
            old = ?old_status,
            new = ?new_status,
            prompt = ?prompt_type,
            "Reconciler: detected prompt, fixing state"
        );

        // Update status counts
        let old_col = status_to_column(&old_status);
        let new_col = status_to_column(&new_status);
        state.status_counts[old_col] = state.status_counts[old_col].saturating_sub(1);
        state.status_counts[new_col] += 1;

        agent.status = new_status;
        agent.last_event = event_name.clone();
        agent.last_update = current_timestamp();

        // Clear orphaned flags
        agent.in_response = false;
        agent.current_tool = None;

        // Add to event log for debug panel visibility
        push_event(
            state,
            synthetic_event(pane_id, &event_name, "attention", &project),
        );

        true
    }

    /// Repair orphaned state fields that block timeout transitions
    fn repair_orphaned_fields(&self, state: &mut AppState, pane_id: &str, now: i64) -> bool {
        let Some(agent) = state.agents.get_mut(pane_id) else {
            return false;
        };

        let elapsed = now - agent.last_update;
        let mut modified = false;

        // Clear orphaned current_tool if too old
        if agent.current_tool.is_some() && elapsed > ORPHANED_TOOL_TIMEOUT_SECS {
            tracing::info!(
                pane_id = %pane_id,
                tool = ?agent.current_tool,
                elapsed_secs = elapsed,
                "Reconciler: clearing orphaned current_tool"
            );
            agent.current_tool = None;
            agent.pending_tool_start = None;
            agent.pending_tool_use_id = None;
            modified = true;
        }

        // Clear orphaned in_response if too old
        if agent.in_response && elapsed > ORPHANED_RESPONSE_TIMEOUT_SECS {
            tracing::info!(
                pane_id = %pane_id,
                elapsed_secs = elapsed,
                "Reconciler: clearing orphaned in_response"
            );
            agent.in_response = false;
            modified = true;
        }

        modified
    }
}

/// Get current Unix timestamp in seconds
fn current_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Create a synthetic event for reconciler actions (appears in Event Log debug panel)
fn synthetic_event(pane_id: &str, event_name: &str, status: &str, project: &str) -> HookEvent {
    HookEvent {
        event: event_name.to_string(),
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
        source: EventSource::Local,
        context_window: None,
        agent_type: None,
        permission_mode: None,
        cwd: None,
        transcript_path: None,
        // TeammateTool fields (v3.0) - not applicable for synthetic events
        team_name: None,
        team_agent_id: None,
        team_agent_name: None,
        team_agent_type: None,
        // Version tracking - not applicable for synthetic events
        claude_code_version: None,
        // Model tracking - not applicable for synthetic events
        model: None,
        // Claude Code 2.1.33 fields - not applicable for synthetic events
        session_source: None,
        stop_hook_active: None,
        agent_transcript_path: None,
        trigger: None,
        // Effort level - not applicable for synthetic events
        effort_level: None,
        // TeammateIdle/TaskCompleted - not applicable for synthetic events
        teammate_name: None,
        task_id: None,
        task_subject: None,
        task_description: None,
        // PostToolUse response - not applicable for synthetic events
        tool_response: None,
    }
}

/// Add event to state's event log (for debug panel visibility)
fn push_event(state: &mut AppState, event: HookEvent) {
    state.events.push_front(event);
    if state.events.len() > MAX_EVENTS {
        state.events.pop_back();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Agent;

    const TEST_THRESHOLD: i64 = 5; // 5 second threshold for tests

    #[test]
    fn test_is_uncertain_working_stale() {
        let mut agent = Agent::new("%0".to_string(), "test".to_string());
        agent.status = Status::Working;
        agent.last_update = current_timestamp() - 10; // 10s ago, > threshold

        assert!(Reconciler::is_uncertain(
            &agent,
            current_timestamp(),
            TEST_THRESHOLD
        ));
    }

    #[test]
    fn test_is_uncertain_working_fresh() {
        let mut agent = Agent::new("%0".to_string(), "test".to_string());
        agent.status = Status::Working;
        agent.last_update = current_timestamp() - 2; // 2s ago, < threshold

        assert!(!Reconciler::is_uncertain(
            &agent,
            current_timestamp(),
            TEST_THRESHOLD
        ));
    }

    #[test]
    fn test_is_uncertain_attention_stale() {
        let mut agent = Agent::new("%0".to_string(), "test".to_string());
        agent.status = Status::Attention(AttentionType::Waiting);
        agent.last_update = current_timestamp() - 100; // 100s ago

        // Attention status should not be uncertain, only Working
        assert!(!Reconciler::is_uncertain(
            &agent,
            current_timestamp(),
            TEST_THRESHOLD
        ));
    }

    #[test]
    fn test_should_run_initial() {
        let reconciler = Reconciler::default();
        // Should not run immediately after creation
        assert!(!reconciler.should_run());
    }

    #[test]
    fn test_should_run_disabled() {
        let config = ReconciliationConfig {
            enabled: false,
            interval_secs: 3,
            uncertain_threshold_secs: 5,
        };
        let reconciler = Reconciler::new(&config);
        // Should never run when disabled
        assert!(!reconciler.should_run());
    }
}
