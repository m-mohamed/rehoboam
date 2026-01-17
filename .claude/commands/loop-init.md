---
description: Initialize and validate Rehoboam loop state directory for autonomous agent iterations
allowed-tools: Bash(mkdir:*), Bash(ls:*), Bash(cat:*), Read, Write
argument-hint: [role: planner|worker|auto]
---

# Loop Mode Initialization

Initialize the `.rehoboam/` directory structure for autonomous loop iterations.

## Directory Structure

Create the following structure:
```
.rehoboam/
├── anchor.md          # Immutable task specification
├── tasks.md           # Task queue (Planner writes, Workers read)
├── progress.md        # Work completed each iteration
├── guardrails.md      # Learned constraints (append-only)
└── state.json         # Iteration state and config
```

## Initialization Steps

1. **Create directory**:
   ```bash
   mkdir -p .rehoboam
   ```

2. **Create anchor.md** (if not exists):
   Ask user for the task goal and create:
   ```markdown
   # Task Anchor

   ## Goal
   [User's task description]

   ## Success Criteria
   - [Criterion 1]
   - [Criterion 2]

   ## Constraints
   - [Any constraints]
   ```

3. **Create tasks.md** (if using Planner/Worker roles):
   ```markdown
   # Task Queue

   ## Pending

   ## In Progress

   ## Completed
   ```

4. **Create progress.md**:
   ```markdown
   # Progress Log

   ## Iteration History
   ```

5. **Create guardrails.md**:
   ```markdown
   # Guardrails

   Learned constraints from previous iterations.
   ```

## Role Selection

Based on `$ARGUMENTS` or ask user:

- **Planner**: Explores codebase, creates tasks in tasks.md, doesn't implement
- **Worker**: Executes single task in isolation, marks tasks complete
- **Auto** (default): Generic prompt, current legacy behavior

## Validation

If `.rehoboam/` already exists, validate:
1. Check all required files exist
2. Parse state.json for iteration count
3. Report current status (iteration number, pending tasks, role)
4. Offer to reset or continue

## Usage

After initialization, spawn a loop agent:
```bash
rehoboam spawn --loop --role [planner|worker|auto]
```
