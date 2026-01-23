//! Role-Specific Prompts (Cursor-aligned)
//!
//! Generates iteration prompts based on the agent's role using a unified template system.
//! Uses Claude Code native Tasks API exclusively.
//!
//! **REQUIRES**: `CLAUDE_CODE_TASK_LIST_ID` environment variable must be set.
//! Agents use TaskCreate/TaskUpdate/TaskList/TaskGet tools for task management.

use color_eyre::eyre::{eyre, Result};
use std::fs;
use std::path::Path;
use tracing::debug;

use super::coordination::{list_workers, read_broadcasts};
use super::state::{load_state, LoopRole, LoopState};

// =============================================================================
// Shared Context Loading
// =============================================================================

/// Get the Claude Code Task List ID, or error if not set
fn require_task_list_id() -> Result<String> {
    std::env::var("CLAUDE_CODE_TASK_LIST_ID").map_err(|_| {
        eyre!("CLAUDE_CODE_TASK_LIST_ID not set. Loop mode requires Claude Code Tasks API.")
    })
}

/// Shared context loaded once for all prompt types
struct PromptContext {
    task_list_id: String,
    iteration: u32,
    stop_word: String,
    anchor: String,
    progress: String,
    guardrails: String,
}

impl PromptContext {
    /// Load all context files once
    fn load(loop_dir: &Path, state: &LoopState) -> Result<Self> {
        Ok(Self {
            task_list_id: require_task_list_id()?,
            iteration: state.iteration + 1,
            stop_word: state.stop_word.clone(),
            anchor: fs::read_to_string(loop_dir.join("anchor.md")).unwrap_or_default(),
            progress: fs::read_to_string(loop_dir.join("progress.md")).unwrap_or_default(),
            guardrails: fs::read_to_string(loop_dir.join("guardrails.md")).unwrap_or_default(),
        })
    }
}

// =============================================================================
// Prompt Templates (Role-Specific Sections)
// =============================================================================

/// Planner-specific rules section
const PLANNER_RULES: &str = r#"## Rules for Planners (Claude Code Tasks API)
1. **Explore first** - Read the codebase to understand structure and patterns
2. **Check existing tasks** - Use `TaskList` before creating new tasks to avoid duplicates
3. Break down the goal into discrete, independent tasks
4. Each task should be completable by a single worker in ONE iteration
5. **Create tasks with TaskCreate** - Use clear subjects and detailed descriptions:
   - subject: Brief imperative title (e.g., "Implement user authentication endpoint")
   - description: Full context a worker needs to complete the task
6. Do NOT implement anything yourself
7. Do NOT coordinate with workers - they work in isolation
8. When planning is complete, write "PLANNING COMPLETE" to progress.md
9. If stuck, add more exploration tasks rather than trying to solve everything
10. Update progress.md with your exploration findings

## Task Guidelines
- **No duplicates** - Use `TaskList` to check before creating; skip if similar task exists
- Tasks should be atomic and independent
- Include enough context in the description for a worker to understand
- Create related tasks in logical order (dependencies first)

Remember: Your job is PLANNING, not IMPLEMENTING. Explore thoroughly, create clear tasks with TaskCreate."#;

/// Worker-specific rules section
const WORKER_RULES: &str = r#"## How to Get Your Task (Claude Code Tasks API)
1. Use `TaskList` to see all pending tasks
2. Pick ONE task that is `pending` with no owner
3. Use `TaskUpdate` to claim it: set status to `in_progress`
4. Complete the task
5. Use `TaskUpdate` to mark it `completed` when done

## Rules for Workers (Claude Code Tasks API)
1. **Claim a task FIRST** - Use `TaskUpdate` with status: "in_progress" before starting
2. Focus ONLY on your claimed task - ignore other work
3. Do NOT coordinate with other workers - they handle their own tasks
4. Do NOT explore unrelated code or add scope
5. When done, use `TaskUpdate` with status: "completed"
6. Update progress.md with what you accomplished
7. If blocked by something outside your task, note it in progress.md and exit
8. Do NOT try to solve blockers that require other tasks
9. Write "{stop_word}" when your task is fully complete

## Task Workflow
```
Use TaskList → Find pending task → TaskUpdate(in_progress) → Do work → TaskUpdate(completed)
```

Remember: Claim ONE task with TaskUpdate, complete it, mark completed, then exit."#;

/// Auto-mode instructions section
const AUTO_RULES: &str = r#"## Instructions for This Iteration
1. Read the anchor to understand your task
2. Check guardrails before taking actions
3. Use `TaskList` to check current tasks and their status
4. Continue from where progress.md left off
5. Update progress.md with your work
6. Use `TaskCreate` to track sub-tasks if needed
7. Use `TaskUpdate` to mark tasks completed as you finish them
8. If you hit a repeating problem, add a SIGN to guardrails.md
9. When ALL criteria are met, write either:
   - "{stop_word}" anywhere in progress.md, OR
   - <promise>COMPLETE</promise> tag (more explicit)
10. Exit when you've made progress (don't try to finish everything)

Remember: Progress persists, failures evaporate. Make incremental progress.
Git commits are created between iterations for easy rollback."#;

// =============================================================================
// Unified Prompt Builder
// =============================================================================

/// Build a planner-specific prompt using shared context
fn build_planner_prompt(ctx: &PromptContext) -> String {
    format!(
        r#"# Rehoboam Loop - PLANNER - Iteration {iteration}

You are a PLANNER. Your job is to explore and decompose work into tasks.

## Task List ID
{task_list_id}

## Your Goal
{anchor}

## Progress So Far
{progress}

{rules}
"#,
        iteration = ctx.iteration,
        task_list_id = ctx.task_list_id,
        anchor = ctx.anchor,
        progress = ctx.progress,
        rules = PLANNER_RULES,
    )
}

/// Build a worker-specific prompt using shared context
fn build_worker_prompt(ctx: &PromptContext) -> String {
    let rules = WORKER_RULES.replace("{stop_word}", &ctx.stop_word);
    format!(
        r#"# Rehoboam Loop - WORKER - Iteration {iteration}

You are a WORKER. Your job is to complete ONE task from the shared task list.

## Task List ID
{task_list_id}

## Context (for reference only)
{anchor}

## Guardrails
{guardrails}

{rules}
"#,
        iteration = ctx.iteration,
        task_list_id = ctx.task_list_id,
        anchor = ctx.anchor,
        guardrails = ctx.guardrails,
        rules = rules,
    )
}

/// Build auto-mode prompt with optional coordination context
fn build_auto_prompt(loop_dir: &Path, ctx: &PromptContext) -> String {
    // Get recent iteration context (last 5 iterations)
    let recent = super::activity::get_recent_progress_summary(loop_dir, 5).unwrap_or_default();
    let recent_section = if recent.is_empty() {
        String::new()
    } else {
        format!("## Recent Activity\n{}\n", recent)
    };

    // Get recent broadcasts from coordination.md
    let broadcasts = read_broadcasts(loop_dir, Some(60)).unwrap_or_default();
    let coordination_section = if broadcasts.is_empty() {
        String::new()
    } else {
        format!(
            "## Coordination (from other workers)\n{}\n\n",
            broadcasts.join("\n")
        )
    };

    // List active workers if any
    let workers = list_workers(loop_dir).unwrap_or_default();
    let workers_section = if workers.is_empty() {
        String::new()
    } else {
        format!(
            "## Active Workers\n{}\n\n",
            workers
                .iter()
                .map(|w| format!("- {}", w))
                .collect::<Vec<_>>()
                .join("\n")
        )
    };

    let rules = AUTO_RULES.replace("{stop_word}", &ctx.stop_word);

    format!(
        r#"# Rehoboam Loop - Iteration {iteration}

You are in a Rehoboam loop. Each iteration starts fresh - make incremental progress.

## Task List ID
{task_list_id}

{recent_section}{coordination_section}{workers_section}## Your Task (Anchor)
{anchor}

## Learned Constraints (Guardrails)
{guardrails}

## Progress So Far
{progress}

{rules}
"#,
        iteration = ctx.iteration,
        task_list_id = ctx.task_list_id,
        recent_section = recent_section,
        coordination_section = coordination_section,
        workers_section = workers_section,
        anchor = ctx.anchor,
        guardrails = ctx.guardrails,
        progress = ctx.progress,
        rules = rules,
    )
}

// =============================================================================
// Public API
// =============================================================================

/// Build the iteration prompt that includes state files
///
/// This creates a prompt file that tells Claude:
/// - What iteration it's on
/// - The anchor (task spec)
/// - Any guardrails
/// - Progress so far
///
/// Uses unified template system with role-specific sections.
pub fn build_iteration_prompt(loop_dir: &Path) -> Result<String> {
    let state = load_state(loop_dir)?;
    let ctx = PromptContext::load(loop_dir, &state)?;

    // Dispatch to role-specific prompts using shared context
    let prompt = match state.role {
        LoopRole::Planner => build_planner_prompt(&ctx),
        LoopRole::Worker => build_worker_prompt(&ctx),
        LoopRole::Auto => build_auto_prompt(loop_dir, &ctx),
    };

    // Write to a temp file for claude stdin piping
    let prompt_file = loop_dir.join("_iteration_prompt.md");
    fs::write(&prompt_file, &prompt)?;

    debug!(
        "Built {} iteration prompt for iteration {}",
        state.role, ctx.iteration
    );
    Ok(prompt_file.to_string_lossy().to_string())
}

/// Build additionalContext string for hook injection (Claude Code 2.1.x)
///
/// This creates a context string that can be returned from a hook to inject
/// additional context into Claude's conversation. Used by `rehoboam hook --inject-context`.
pub fn build_loop_context(loop_dir: &Path) -> Result<String> {
    let state = load_state(loop_dir)?;
    let anchor = std::fs::read_to_string(loop_dir.join("anchor.md")).unwrap_or_default();
    let progress = std::fs::read_to_string(loop_dir.join("progress.md")).unwrap_or_default();
    let guardrails = std::fs::read_to_string(loop_dir.join("guardrails.md")).unwrap_or_default();

    // Get task list ID if set
    let task_list_id = std::env::var("CLAUDE_CODE_TASK_LIST_ID").unwrap_or_default();

    // Build context based on role
    let role_context = match state.role {
        LoopRole::Planner => {
            "You are a PLANNER. Explore and create tasks with TaskCreate, do NOT implement."
        }
        LoopRole::Worker => {
            "You are a WORKER. Use TaskList to find tasks, claim with TaskUpdate, then execute."
        }
        LoopRole::Auto => {
            "You are in autonomous loop mode. Use TaskCreate/TaskUpdate to track progress."
        }
    };

    Ok(format!(
        r"## Rehoboam Loop Context

**Iteration:** {}/{} | **Role:** {} | **Stop Word:** {}
**Task List ID:** {}

{}

### Task (anchor.md)
{}

### Progress (progress.md)
{}

### Guardrails
{}

Use `TaskList` to see current tasks. Use `TaskCreate`/`TaskUpdate` to manage tasks.
",
        state.iteration + 1,
        state.max_iterations,
        state.role,
        state.stop_word,
        task_list_id,
        role_context,
        anchor.trim(),
        progress.trim(),
        guardrails.trim()
    ))
}
