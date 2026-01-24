# Rehoboam's Loop Specification

Rehoboam's Loop is an autonomous iteration system for long-running coding tasks.

## Overview

Rehoboam's Loop enables:
- **Persistent iteration state** - Work survives across sessions
- **Heuristic-based completion detection** - Judge evaluates progress
- **Fresh session spawning** - Context never degrades
- **TeammateTool integration** - Roles and coordination via Claude Code

## Architecture

### Agent Roles

Agent roles are now managed by Claude Code's TeammateTool:

| Source | Environment Variable | Description |
|--------|---------------------|-------------|
| TeammateTool | `CLAUDE_CODE_AGENT_TYPE` | Explicit role assignment |
| Rehoboam | AgentRole enum | Inferred from tool usage (observability) |

Rehoboam monitors both explicit types and observed behavior for anomaly detection.

### State Files (.rehoboam/)

Each loop maintains state in the `.rehoboam/` directory:

| File | Purpose | Mutability |
|------|---------|------------|
| `anchor.md` | Task specification | Immutable (set at spawn) |
| `progress.md` | Cumulative work completed | Append-only |
| `guardrails.md` | Learned constraints from errors | Append-only |
| `state.json` | Iteration counter, config, timing | Managed by system |
| `activity.log` | Iteration timing and metrics | Append-only |
| `session_history.log` | State transitions for debugging | Append-only |

**Note:** `tasks.md` and `coordination.md` are no longer created. Tasks are managed via Claude Code's Tasks API (TaskCreate/TaskUpdate/TaskList/TaskGet). Coordination is handled by TeammateTool's `write`/`broadcast` operations.

### Judge System

The judge evaluates completion to decide whether to continue iteration:

```rust
pub enum JudgeDecision {
    Continue,   // More work needed
    Complete,   // All goals achieved
    Stalled,    // Progress blocked
}
```

**Detection Heuristics:**

| Pattern | Decision |
|---------|----------|
| "all tasks completed" | Complete |
| "implementation complete" | Complete |
| Stop word found | Complete |
| `<promise>` tag found | Complete |
| "blocked by", "stuck on" | Stalled |
| 5 identical Stop reasons | Stalled |
| Max iterations reached | Stalled |
| Otherwise | Continue |

## How to Use

### Spawning a Loop

1. Press `s` to open spawn dialog
2. Enable "Loop Mode" checkbox
3. Set max iterations and stop word
4. Write task prompt with clear goals
5. Start loop

### Monitor Progress

- **TUI Dashboard** (`d`) - Shows iteration counts, completion status
- **progress.md** - Human-readable progress log
- **Loop indicator** - Shows iteration count in agent card

### Stop Conditions

Loops stop when:
1. Stop word detected in progress.md
2. `<promise>` tag detected
3. Max iterations reached (default: 50)
4. 5 consecutive identical Stop reasons (stalled)
5. Judge determines "Complete" or "Stalled"
6. Manual `n` (reject) on Stop event

## TeammateTool Integration

Rehoboam monitors TeammateTool operations for TUI display:

| TeammateTool Operation | Rehoboam Monitoring |
|------------------------|---------------------|
| `CLAUDE_CODE_TEAM_NAME` | Team name in header |
| `CLAUDE_CODE_AGENT_TYPE` | Agent type in card |
| `spawnTeam` events | Team member tracking |
| `requestShutdown` | Shutdown status display |

## API Reference

### Rehoboam Loop Module (src/rehoboam_loop/)

**State Management:**
```rust
fn init_loop_dir(work_dir: &Path, anchor: &str, config: &RehoboamConfig) -> Result<PathBuf>
fn load_state(loop_dir: &Path) -> Result<LoopState>
fn save_state(loop_dir: &Path, state: &LoopState) -> Result<()>
fn increment_iteration(loop_dir: &Path) -> Result<u32>
```

**Completion Detection:**
```rust
fn check_completion(loop_dir: &Path, stop_word: &str) -> Result<(bool, String)>
fn check_stop_word(loop_dir: &Path, stop_word: &str) -> Result<bool>
fn check_max_iterations(loop_dir: &Path) -> Result<bool>
```

**Prompts:**
```rust
fn build_iteration_prompt(loop_dir: &Path) -> Result<String>
```

**Judge:**
```rust
fn judge_completion(loop_dir: &Path, last_event_reason: &str) -> JudgeDecision
```

**Activity Logging:**
```rust
fn log_activity(loop_dir: &Path, iteration: u32, duration_secs: Option<u64>, tool_calls: Option<u32>, outcome: &str) -> Result<()>
fn log_session_transition(loop_dir: &Path, from: &str, to: &str, note: Option<&str>) -> Result<()>
fn track_error_pattern(loop_dir: &Path, error: &str) -> Result<bool>
```

## Related Files

| File | Description |
|------|-------------|
| `src/rehoboam_loop/` | Core loop logic, state, activity logging, judge |
| `src/state/agent.rs` | AgentRole inference (observability) |
| `src/app/spawn.rs` | Spawn dialog with loop config |
