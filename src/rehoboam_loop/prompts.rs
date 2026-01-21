//! Role-Specific Prompts (Cursor-aligned)
//!
//! Generates iteration prompts based on the agent's role.

use color_eyre::eyre::Result;
use std::fs;
use std::path::Path;
use tracing::debug;

use super::coordination::{list_workers, read_broadcasts};
use super::state::{load_state, LoopRole, LoopState};
use super::tasks::read_next_task;

/// Build a planner-specific prompt
///
/// Planners explore, decompose tasks, and write to tasks.md.
/// They do NOT implement anything themselves.
fn build_planner_prompt(loop_dir: &Path, state: &LoopState) -> Result<String> {
    let anchor = fs::read_to_string(loop_dir.join("anchor.md")).unwrap_or_default();
    let progress = fs::read_to_string(loop_dir.join("progress.md")).unwrap_or_default();
    let tasks = fs::read_to_string(loop_dir.join("tasks.md")).unwrap_or_default();

    let prompt = format!(
        r#"# Rehoboam Loop - PLANNER - Iteration {iteration}

You are a PLANNER. Your job is to explore and decompose work into tasks.

## Your Goal
{anchor}

## Current Tasks Queue
{tasks}

## Progress So Far
{progress}

## Rules for Planners
1. **Explore first** - Read the codebase to understand structure and patterns
2. **Check existing tasks** - Read tasks.md before creating new tasks to avoid duplicates
3. Break down the goal into discrete, independent tasks
4. Each task should be completable by a single worker in ONE iteration
5. Write tasks to tasks.md in the Pending section using format:
   `- [ ] [TASK-XXX] Description of the task`
6. Do NOT implement anything yourself
7. Do NOT coordinate with workers - they work in isolation
8. When planning is complete, write "PLANNING COMPLETE" to progress.md
9. If stuck, add more exploration tasks rather than trying to solve everything
10. Update progress.md with your exploration findings

## Task Guidelines
- **No duplicates** - Check tasks.md before adding; skip if similar task exists
- Tasks should be atomic and independent
- Include enough context in the description for a worker to understand
- Prefix related tasks with common identifiers (e.g., TASK-AUTH-001, TASK-AUTH-002)
- Order tasks by dependency (simpler tasks first)
- Mark your exploration progress in progress.md so future iterations don't repeat

Remember: Your job is PLANNING, not IMPLEMENTING. Explore thoroughly, create clear tasks.
"#,
        iteration = state.iteration + 1,
        anchor = anchor,
        tasks = tasks,
        progress = progress,
    );

    Ok(prompt)
}

/// Build a worker-specific prompt
///
/// Workers pick ONE task from the queue, execute it in isolation,
/// and mark it complete. They do NOT coordinate with other workers.
///
/// Two modes:
/// 1. **Auto-spawned workers** (Cursor isolation model): Read from assigned_task.md
///    - Task is pre-assigned during worker spawn
///    - No shared tasks.md - complete isolation
/// 2. **Manual workers**: Read from shared tasks.md and claim next task
fn build_worker_prompt(loop_dir: &Path, state: &LoopState) -> Result<String> {
    let anchor = fs::read_to_string(loop_dir.join("anchor.md")).unwrap_or_default();
    let guardrails = fs::read_to_string(loop_dir.join("guardrails.md")).unwrap_or_default();

    // Check for assigned_task.md (auto-spawned isolated worker)
    let assigned_task_path = loop_dir.join("assigned_task.md");
    let (task_section, is_isolated) = if assigned_task_path.exists() {
        // Isolated worker mode: read pre-assigned task
        let assigned_task = fs::read_to_string(&assigned_task_path).unwrap_or_default();
        (assigned_task, true)
    } else {
        // Manual worker mode: read from shared tasks.md
        let next_task = read_next_task(loop_dir)?;
        let section = if let Some(task) = &next_task {
            format!(
                "## Your Assigned Task\n**[{}]** {}\n",
                task.id, task.description
            )
        } else {
            "## Your Assigned Task\nNo tasks available in queue. Check with planner or wait.\n"
                .to_string()
        };
        (section, false)
    };

    // Different rules for isolated vs. shared workers
    let rules = if is_isolated {
        format!(
            r#"## Rules for Workers (Isolated Mode)
1. Focus ONLY on your assigned task - you have ONE task
2. Do NOT coordinate with other workers - they work in isolation
3. Do NOT explore unrelated code or add scope
4. Update progress.md with what you accomplished
5. If blocked, note it in progress.md and write "{stop_word}"
6. When done, write "{stop_word}" to progress.md

Remember: One task, complete isolation, then exit."#,
            stop_word = state.stop_word
        )
    } else {
        format!(
            r#"## Rules for Workers
1. **Mark task "In Progress" FIRST** - Before starting, move your task from Pending to In Progress:
   `- [~] [TASK-XXX] description (worker: YOUR_ID)`
2. Focus ONLY on your assigned task - ignore other work
3. Do NOT coordinate with other workers - they handle their own tasks
4. Do NOT explore unrelated code or add scope
5. When done, **mark task "Completed"** - Move from In Progress to Completed section:
   `- [x] [TASK-XXX] description`
6. Update progress.md with what you accomplished
7. If blocked by something outside your task, note it in progress.md and exit
8. Do NOT try to solve blockers that require other tasks
9. Write "{stop_word}" when your task is fully complete

## Task Status Flow
```
Pending → In Progress → Completed
- [ ]   → - [~]       → - [x]
```

## Task Completion Checklist
- [ ] Task marked "In Progress" at start
- [ ] Task implemented as described
- [ ] Tests pass (if applicable)
- [ ] Task marked "Completed" in tasks.md
- [ ] Progress.md updated with summary
- [ ] Stop word written if fully complete

Remember: Complete YOUR task, then exit. Don't do extra work."#,
            stop_word = state.stop_word
        )
    };

    let prompt = format!(
        r#"# Rehoboam Loop - WORKER - Iteration {iteration}

You are a WORKER. Your job is to complete ONE assigned task.

{task_section}
## Context (for reference only)
{anchor}

## Guardrails
{guardrails}

{rules}
"#,
        iteration = state.iteration + 1,
        task_section = task_section,
        anchor = anchor,
        guardrails = guardrails,
        rules = rules,
    );

    Ok(prompt)
}

/// Build the legacy "Auto" prompt (backward compatible)
fn build_auto_prompt(loop_dir: &Path, state: &LoopState) -> Result<String> {
    let anchor = fs::read_to_string(loop_dir.join("anchor.md")).unwrap_or_default();
    let guardrails = fs::read_to_string(loop_dir.join("guardrails.md")).unwrap_or_default();
    let progress = fs::read_to_string(loop_dir.join("progress.md")).unwrap_or_default();

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

    let prompt = format!(
        r#"# Rehoboam Loop - Iteration {iteration}

You are in a Rehoboam loop. Each iteration starts fresh - make incremental progress.

{recent_section}{coordination_section}{workers_section}## Your Task (Anchor)
{anchor}

## Learned Constraints (Guardrails)
{guardrails}

## Progress So Far
{progress}

## Instructions for This Iteration
1. Read the anchor to understand your task
2. Check guardrails before taking actions
3. Check coordination section for discoveries from other workers
4. Continue from where progress.md left off
5. Update progress.md with your work
6. If you discover something useful for other workers, use broadcast
7. If you hit a repeating problem, add a SIGN to guardrails.md
8. When ALL criteria are met, write either:
   - "{stop_word}" anywhere in progress.md, OR
   - <promise>COMPLETE</promise> tag (more explicit)
9. Exit when you've made progress (don't try to finish everything)

Remember: Progress persists, failures evaporate. Make incremental progress.
Git commits are created between iterations for easy rollback.
"#,
        iteration = state.iteration + 1, // Display as 1-indexed
        recent_section = recent_section,
        coordination_section = coordination_section,
        workers_section = workers_section,
        anchor = anchor,
        guardrails = guardrails,
        progress = progress,
        stop_word = state.stop_word,
    );

    Ok(prompt)
}

/// Build the iteration prompt that includes state files
///
/// This creates a prompt file that tells Claude:
/// - What iteration it's on
/// - The anchor (task spec)
/// - Any guardrails
/// - Progress so far
///
/// Dispatches to role-specific prompts based on state.role
pub fn build_iteration_prompt(loop_dir: &Path) -> Result<String> {
    let state = load_state(loop_dir)?;

    // Dispatch to role-specific prompts
    let prompt = match state.role {
        LoopRole::Planner => build_planner_prompt(loop_dir, &state)?,
        LoopRole::Worker => build_worker_prompt(loop_dir, &state)?,
        LoopRole::Auto => build_auto_prompt(loop_dir, &state)?,
    };

    // Write to a temp file for claude stdin piping
    let prompt_file = loop_dir.join("_iteration_prompt.md");
    fs::write(&prompt_file, &prompt)?;

    debug!(
        "Built {} iteration prompt for iteration {}",
        state.role,
        state.iteration + 1
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
    let tasks = std::fs::read_to_string(loop_dir.join("tasks.md")).unwrap_or_default();

    // Build context based on role
    let role_context = match state.role {
        LoopRole::Planner => "You are a PLANNER. Explore and create tasks, do NOT implement.",
        LoopRole::Worker => "You are a WORKER. Execute ONE assigned task, then exit.",
        LoopRole::Auto => "You are in autonomous loop mode. Make incremental progress.",
    };

    Ok(format!(
        r"## Rehoboam Loop Context

**Iteration:** {}/{} | **Role:** {} | **Stop Word:** {}

{}

### Task (anchor.md)
{}

### Progress (progress.md)
{}

### Tasks Queue (tasks.md)
{}

### Guardrails
{}
",
        state.iteration + 1,
        state.max_iterations,
        state.role,
        state.stop_word,
        role_context,
        anchor.trim(),
        progress.trim(),
        tasks.trim(),
        guardrails.trim()
    ))
}
