# Rehoboam's Loop Specification

Rehoboam's Loop is an autonomous iteration system for long-running coding tasks, inspired by Cursor's "Scaling Agents" architecture.

## Overview

Rehoboam's Loop enables:
- **Hierarchical agent roles** - Planners explore, Workers execute
- **Persistent task queues** - Work survives across iterations
- **Heuristic-based completion detection** - Judge evaluates progress
- **Fresh session spawning** - Context never degrades

## Cursor Scaling Agents Insights

Key learnings from Cursor's engineering blog that informed this design:

| Insight | Implementation |
|---------|----------------|
| Planners explore, Workers execute | `LoopRole::Planner` and `LoopRole::Worker` roles |
| Workers never coordinate | Each Worker gets ONE task in isolation |
| Judge evaluates progress | `judge_completion()` heuristic in rehoboam_loop.rs |
| Fresh sessions per iteration | Each iteration spawns new Claude session |
| Prompting matters more than harness | Role-specific prompts in `build_*_prompt()` |

## Architecture

### Roles (LoopRole enum)

```rust
pub enum LoopRole {
    Planner,  // Explores codebase, creates tasks in tasks.md
    Worker,   // Claims single task, executes, marks complete
    Auto,     // Legacy generic behavior (backwards compatibility)
}
```

**Planner:**
- Reads anchor.md to understand the goal
- Explores codebase to identify required work
- Writes discrete tasks to tasks.md
- Does NOT implement anything
- Writes "PLANNING COMPLETE" when done

**Worker:**
- Assigned ONE task from tasks.md
- Marks task "In Progress" before starting
- Implements the task in isolation
- Marks task "Completed" when done
- Updates progress.md with summary

**Auto:**
- Legacy behavior for backwards compatibility
- Generic iteration-based approach
- Suitable for single-agent loops

### State Files (.rehoboam/)

Each loop maintains state in the `.rehoboam/` directory:

| File | Purpose | Mutability |
|------|---------|------------|
| `anchor.md` | Task specification | Immutable (set at spawn) |
| `progress.md` | Cumulative work completed | Append-only |
| `tasks.md` | Task queue (Pending/In Progress/Completed) | Managed by agents |
| `guardrails.md` | Learned constraints from errors | Append-only |
| `state.json` | Iteration counter, config, timing | Managed by system |
| `coordination.md` | Multi-agent messaging (opt-in) | Append-only |

### Task Queue Format (tasks.md)

```markdown
## Pending
- [ ] [TASK-001] Set up project structure
- [ ] [TASK-002] Implement user authentication

## In Progress
- [~] [TASK-003] Create API endpoints (worker: %42)

## Completed
- [x] [TASK-000] Initialize repository
```

**Task States:**
- `[ ]` - Pending (unclaimed)
- `[~]` - In Progress (claimed by worker)
- `[x]` - Completed

### Judge System

The judge evaluates each Stop event to decide whether to continue iteration:

```rust
pub enum JudgeDecision {
    Continue,   // More work needed
    Complete,   // All goals achieved
    Stalled,    // Progress blocked
}
```

**Heuristic Detection:**

| Pattern | Decision |
|---------|----------|
| "all tasks completed" | Complete |
| "implementation complete" | Complete |
| Stop word found | Complete |
| "blocked by", "stuck on" | Stalled |
| 5 identical Stop reasons | Stalled |
| Max iterations reached | Stalled |
| Otherwise | Continue |

**Custom Judge Prompt:**
Set via spawn dialog to provide domain-specific completion criteria:
```
Check if all API endpoints return valid responses and tests pass
```

## How to Use

### Spawning a Planner Loop

1. Press `s` to open spawn dialog
2. Enable "Loop Mode" checkbox
3. Set Role to **Planner**
4. Write anchor.md with clear goals
5. Start loop - Planner will explore and populate tasks.md

### Spawning Worker Loops

1. Wait for Planner to create tasks in tasks.md
2. Press `s` to spawn new agent
3. Enable "Loop Mode", set Role to **Worker**
4. Repeat for parallel Workers (each claims different task)
5. Workers execute in isolation until queue empty

### Monitor Progress

- **TUI Dashboard** (`d`) - Shows iteration counts, completion status
- **progress.md** - Human-readable progress log
- **tasks.md** - Current queue state
- **Loop indicator** - Shows iteration count in agent card

### Stop Conditions

Loops stop when:
1. Stop word detected in Stop event reason
2. Max iterations reached (default: 10)
3. 5 consecutive identical Stop reasons (stalled)
4. Judge determines "Complete" or "Stalled"
5. Manual `n` (reject) on Stop event

## Current Limitations

| Limitation | Description |
|------------|-------------|
| Judge is heuristic-only | No LLM-based evaluation yet |
| Task claiming is manual | Agents manage tasks.md via prompts |
| No role enforcement | Workers could theoretically explore |
| Single-machine primary | Multi-sprite coordination is basic |

## Roadmap (Future Work)

### Phase 1: Judge Enhancement
- LLM-based judge with custom prompts
- Separate Claude session for evaluation
- Feedback loop for judge improvement

### Phase 2: Task Automation
- Auto-claim task on Worker iteration start
- Task queue status in TUI header
- Conflict detection (two workers same task)

### Phase 3: Distributed Execution
- Role-based sprite allocation (N planners, M workers)
- Load balancing between workers
- Result aggregation and merge strategies

### Phase 4: Role Enforcement
- Prevent Workers from using exploration tools
- Prevent Planners from using mutation tools
- Guardrails for tool usage by role

## API Reference

### Rehoboam Loop Module (rehoboam_loop.rs)

**State Management:**
```rust
fn init_loop_dir(work_dir: &Path, anchor: &str) -> Result<PathBuf>
fn load_state(loop_dir: &Path) -> Result<LoopState>
fn save_state(loop_dir: &Path, state: &LoopState) -> Result<()>
```

**Task Queue:**
```rust
fn read_pending_tasks(loop_dir: &Path) -> Result<Vec<Task>>
fn read_next_task(loop_dir: &Path) -> Result<Option<Task>>
fn claim_task(loop_dir: &Path, task_id: &str, worker_id: &str) -> Result<()>
fn complete_task(loop_dir: &Path, task_id: &str) -> Result<()>
fn add_task(loop_dir: &Path, task_id: &str, description: &str) -> Result<()>
```

**Prompts:**
```rust
fn build_iteration_prompt(loop_dir: &Path) -> Result<String>
fn build_planner_prompt(loop_dir: &Path, state: &LoopState) -> Result<String>
fn build_worker_prompt(loop_dir: &Path, state: &LoopState) -> Result<String>
```

**Judge:**
```rust
fn judge_completion(reason: &str, agent: &Agent) -> JudgeResult
```

## Related Files

| File | Description |
|------|-------------|
| `src/rehoboam_loop.rs` | Core loop logic, task queue, prompts |
| `src/state/mod.rs` | `LoopConfig`, `JudgeDecision` types |
| `src/state/agent.rs` | `LoopMode`, `LoopRole` enums |
| `src/app/spawn.rs` | Spawn dialog with loop config |
| `ARCHITECTURE.md` | System architecture overview |
