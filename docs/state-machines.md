# Rehoboam State Machines

This document describes the state machines and flows in Rehoboam, including the auto-spawn workers feature.

## 1. Loop Mode State Machine

```
                    ┌─────────────────────────────────────────┐
                    │                                         │
                    ▼                                         │
              ┌──────────┐                                    │
              │   None   │ ◄─── Agent not in loop mode        │
              └────┬─────┘                                    │
                   │                                          │
                   │ spawn_agent(loop_enabled=true)           │
                   │ → registers pending_loop_config          │
                   ▼                                          │
              ┌──────────┐                                    │
              │  Active  │ ◄─── Loop running                  │
              └────┬─────┘                                    │
                   │                                          │
      ┌────────────┼────────────┬────────────────┐            │
      │            │            │                │            │
      ▼            ▼            ▼                ▼            │
 ┌─────────┐ ┌─────────┐ ┌──────────┐    ┌───────────┐       │
 │ Complete│ │ Stalled │ │ Continue │    │ Cancelled │       │
 │(stop_   │ │(5+ same │ │(Judge    │    │(user X key│       │
 │word/max)│ │reasons) │ │says ok)  │    │          )│       │
 └─────────┘ └─────────┘ └────┬─────┘    └───────────┘       │
                              │                               │
                              │ spawn_fresh_session()         │
                              │ OR spawn_worker_pool()        │
                              │                               │
                              └───────────────────────────────┘
```

## 2. Event Processing Flow (Stop Event)

```
                              ┌──────────────────┐
                              │  Stop Event      │
                              │  Received        │
                              └────────┬─────────┘
                                       │
                          ┌────────────▼────────────┐
                          │ agent.loop_mode ==      │
                          │       Active?           │
                          └────────────┬────────────┘
                                       │
                    ┌──────────────────┼──────────────────┐
                    │ No               │ Yes              │
                    ▼                  ▼                  │
              ┌──────────┐     ┌──────────────┐           │
              │ Normal   │     │ Increment    │           │
              │ status   │     │ iteration    │           │
              │ update   │     └──────┬───────┘           │
              └──────────┘            │                   │
                                      ▼                   │
                           ┌──────────────────┐           │
                           │ Circuit Breakers │           │
                           └─────────┬────────┘           │
                                     │                    │
          ┌──────────────────────────┼──────────────────────────┐
          │                          │                          │
          ▼                          ▼                          ▼
   ┌─────────────┐           ┌─────────────┐            ┌─────────────┐
   │ Max iter    │           │ Stop word   │            │ 5+ identical│
   │ reached?    │           │ found?      │            │ stop reason?│
   └──────┬──────┘           └──────┬──────┘            └──────┬──────┘
          │ Yes                     │ Yes                      │ Yes
          ▼                         ▼                          ▼
   ┌─────────────┐           ┌─────────────┐            ┌─────────────┐
   │ Complete    │           │ Complete    │            │ Stalled     │
   └─────────────┘           └─────────────┘            └─────────────┘

          │ No (all pass)
          ▼
   ┌─────────────────────────────────────────┐
   │           Judge Evaluation              │
   │   run_llm_judge(loop_dir) via Claude    │
   │                                         │
   │   Reads: anchor.md + progress.md        │
   │   Returns: CONTINUE | COMPLETE | STALLED│
   └─────────────────┬───────────────────────┘
                     │
      ┌──────────────┼──────────────┐
      │              │              │
      ▼              ▼              ▼
 ┌─────────┐   ┌──────────┐   ┌─────────┐
 │CONTINUE │   │ COMPLETE │   │ STALLED │
 └────┬────┘   └──────────┘   └─────────┘
      │
      ▼
┌─────────────────────────────────────────┐
│  Is Planner with auto_spawn_workers?    │
│  AND is_planning_complete()?            │
│  AND pending_tasks > 0?                 │
└─────────────────┬───────────────────────┘
                  │
       ┌──────────┴──────────┐
       │ Yes                 │ No
       ▼                     ▼
┌──────────────┐      ┌──────────────────┐
│ Auto-Spawn   │      │ Fresh Session    │
│ Workers      │      │ (normal loop)    │
└──────────────┘      └──────────────────┘
```

## 3. Auto-Spawn Workers Flow

```
┌─────────────────────────────────────────────────────────────────┐
│                    PLANNER COMPLETES                            │
│                                                                 │
│  progress.md contains "PLANNING COMPLETE"                       │
│  tasks.md has pending tasks                                     │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│                spawn_worker_pool_for_planner()                  │
│                                                                 │
│  1. Read pending tasks from tasks.md                            │
│  2. to_spawn = min(pending_tasks.len(), max_workers)            │
│  3. For each task (with 1s delay between):                      │
│     └─► spawn_worker(task)                                      │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│                    spawn_worker(task)                           │
│                                                                 │
│  1. Create worker_dir: .rehoboam-worker-{TASK_ID}/              │
│  2. init_worker_dir():                                          │
│     ├─► Copy anchor.md, guardrails.md from parent               │
│     ├─► Create assigned_task.md (ONE task only)                 │
│     ├─► Create state.json (role=Worker, assigned_task=ID)       │
│     └─► Create progress.md, empty logs                          │
│  3. build_iteration_prompt(worker_dir)                          │
│  4. TmuxController::respawn_claude_with_loop_dir()              │
│     └─► Sets REHOBOAM_LOOP_DIR environment variable             │
│  5. Return WorkerInfo {pane_id, task_id, worker_loop_dir}       │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│                   Register Workers with TUI                      │
│                                                                 │
│  For each WorkerInfo:                                           │
│    register_loop_config(                                        │
│      pane_id,                                                   │
│      max_iterations=10,                                         │
│      stop_word="DONE",                                          │
│      loop_dir=worker_loop_dir,                                  │
│      auto_spawn_workers=false,                                  │
│      max_workers=0,                                             │
│      role=Worker                                                │
│    )                                                            │
└─────────────────────────────────────────────────────────────────┘
```

## 4. Worker Isolation Model (Cursor-aligned)

```
┌─────────────────────────────────────────────────────────────────┐
│                        PROJECT DIRECTORY                        │
│                                                                 │
│  ┌─────────────────┐                                            │
│  │   .rehoboam/    │ ◄─── Planner's loop directory              │
│  │                 │                                            │
│  │  ├─ anchor.md   │      (immutable task spec)                 │
│  │  ├─ progress.md │      (Planner writes "PLANNING COMPLETE")  │
│  │  ├─ tasks.md    │      (Planner creates tasks here)          │
│  │  ├─ guardrails  │                                            │
│  │  └─ state.json  │      (role: Planner)                       │
│  └─────────────────┘                                            │
│                                                                 │
│  ┌───────────────────────────┐   ┌───────────────────────────┐  │
│  │ .rehoboam-worker-task-001/│   │ .rehoboam-worker-task-002/│  │
│  │                           │   │                           │  │
│  │  ├─ anchor.md (copy)      │   │  ├─ anchor.md (copy)      │  │
│  │  ├─ guardrails.md (copy)  │   │  ├─ guardrails.md (copy)  │  │
│  │  ├─ assigned_task.md ◄────┤   │  ├─ assigned_task.md ◄────│  │
│  │  │   (ONLY TASK-001)      │   │  │   (ONLY TASK-002)      │  │
│  │  ├─ progress.md           │   │  ├─ progress.md           │  │
│  │  └─ state.json            │   │  └─ state.json            │  │
│  │      (role: Worker)       │   │      (role: Worker)       │  │
│  │      (assigned: TASK-001) │   │      (assigned: TASK-002) │  │
│  └───────────────────────────┘   └───────────────────────────┘  │
│                                                                 │
│  KEY ISOLATION POINTS:                                          │
│  • Workers do NOT share tasks.md (no race conditions)           │
│  • Workers do NOT see other workers' directories                │
│  • Each worker has REHOBOAM_LOOP_DIR env var set                │
│  • Workers write "DONE" to their own progress.md                │
└─────────────────────────────────────────────────────────────────┘
```

## 5. Initialization Flow

### 5.1 New Project (First Time)

```
User runs: rehoboam spawn --loop --prompt "Build a REST API"

┌─────────────────────────────────────────────────────────────────┐
│                     spawn_tmux_agent()                          │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│                   init_loop_dir() called                        │
│                                                                 │
│  Creates .rehoboam/ with:                                       │
│  • anchor.md (task: "Build a REST API")                         │
│  • guardrails.md (empty)                                        │
│  • progress.md ("Starting iteration 1...")                      │
│  • tasks.md (empty sections)                                    │
│  • state.json (iteration: 0, role: based on selection)          │
│  • errors.log, activity.log, session_history.log (empty)        │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│               register_loop_config(pane_id, ...)                │
│                                                                 │
│  Stores pending config in self.pending_loop_configs             │
│  Config applied when agent sends first hook event               │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│                 start_claude_in_pane()                          │
│                                                                 │
│  Executes: cat {prompt_file} | claude                           │
│  Claude starts working, sends hook events                       │
└─────────────────────────────────────────────────────────────────┘
```

### 5.2 Existing Project (Resuming/Re-entering)

```
User runs: rehoboam spawn --loop --prompt "Continue work"
(Project already has .rehoboam/ from previous session)

┌─────────────────────────────────────────────────────────────────┐
│                     spawn_tmux_agent()                          │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│                   init_loop_dir() called                        │
│                                                                 │
│  fs::create_dir_all(.rehoboam/) - OK if exists                  │
│                                                                 │
│  OVERWRITES:                                                    │
│  • anchor.md (new prompt)                                       │
│  • guardrails.md (reset to empty)                               │
│  • progress.md (reset to "Starting iteration 1...")             │
│  • tasks.md (reset to empty)                                    │
│  • state.json (iteration: 0, fresh start)                       │
│  • errors.log, activity.log, session_history.log (reset)        │
│                                                                 │
│  NOTE: This is a FRESH START, not a resume!                     │
│  To resume, use the existing prompt or modify manually.         │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
                    (same as new project)
```

### 5.3 Worker Spawned by Auto-Spawn

```
Planner completes → triggers spawn_worker_pool_for_planner()

┌─────────────────────────────────────────────────────────────────┐
│                     spawn_worker(task)                          │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│                   init_worker_dir() called                      │
│                                                                 │
│  Creates .rehoboam-worker-{task_id}/ with:                      │
│  • anchor.md (COPIED from parent .rehoboam/)                    │
│  • guardrails.md (COPIED from parent)                           │
│  • assigned_task.md (ONLY this worker's task)                   │
│  • progress.md ("Working on: [TASK-XXX]...")                    │
│  • state.json (role: Worker, assigned_task: TASK-XXX)           │
│  • errors.log, activity.log, session_history.log (empty)        │
│                                                                 │
│  NO tasks.md - workers are isolated!                            │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│        TmuxController::respawn_claude_with_loop_dir()           │
│                                                                 │
│  Command: export REHOBOAM_LOOP_DIR='{worker_dir}' &&            │
│           cat '{prompt_file}' | claude                          │
│                                                                 │
│  The REHOBOAM_LOOP_DIR ensures find_rehoboam_dir() returns      │
│  the worker's isolated directory, not the parent .rehoboam/     │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│               register_loop_config(worker_pane_id, ...)         │
│                                                                 │
│  Stores pending config for worker                               │
│  Worker's hooks will find their isolated loop_dir via env var   │
└─────────────────────────────────────────────────────────────────┘
```

## 6. Hook Event Flow (Worker vs Planner)

```
┌─────────────────────────────────────────────────────────────────┐
│                 Claude Code sends hook event                    │
│                 (PreToolUse, PermissionRequest, Stop, etc.)     │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│                  rehoboam hook (main.rs)                        │
│                                                                 │
│  1. Parse JSON from stdin                                       │
│  2. Enrich with terminal context                                │
│  3. find_rehoboam_dir() called:                                 │
│     ├─► Check REHOBOAM_LOOP_DIR env var (workers)               │
│     └─► Fall back to .rehoboam/ search (planners)               │
└────────────────────────────┬────────────────────────────────────┘
                             │
          ┌──────────────────┴──────────────────┐
          │                                     │
          ▼                                     ▼
┌─────────────────────┐             ┌─────────────────────┐
│ Worker Session      │             │ Planner Session     │
│                     │             │                     │
│ REHOBOAM_LOOP_DIR   │             │ No env var set      │
│ points to:          │             │                     │
│ .rehoboam-worker-X/ │             │ find_rehoboam_dir() │
│                     │             │ finds: .rehoboam/   │
└─────────────────────┘             └─────────────────────┘
          │                                     │
          │                                     │
          └──────────────────┬──────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│                 Send to TUI via Unix socket                     │
│                                                                 │
│  HookEvent includes pane_id for routing                         │
│  TUI looks up pending_loop_config by pane_id                    │
│  Applies loop config to agent (loop_dir, role, etc.)            │
└─────────────────────────────────────────────────────────────────┘
```

## 7. Full Lifecycle Example

```
1. USER: rehoboam spawn --loop --role Planner
   └─► Creates .rehoboam/, starts Planner

2. PLANNER explores codebase
   └─► Reads files, writes tasks to tasks.md

3. PLANNER writes "PLANNING COMPLETE" to progress.md
   └─► Stop event triggers

4. TUI: process_event() handles Stop
   ├─► Judge returns CONTINUE
   ├─► is_planning_complete() = true
   ├─► pending_tasks > 0
   └─► spawn_worker_pool_for_planner()

5. WORKERS spawned (up to max_workers)
   ├─► Worker 1: .rehoboam-worker-task-001/
   ├─► Worker 2: .rehoboam-worker-task-002/
   └─► Worker 3: .rehoboam-worker-task-003/

6. Each WORKER:
   ├─► Reads assigned_task.md
   ├─► Implements their task
   ├─► Writes "DONE" to progress.md
   └─► Stop event triggers

7. TUI: For each worker Stop event
   ├─► Judge evaluates completion
   ├─► If COMPLETE: worker.loop_mode = Complete
   └─► If CONTINUE: spawn_fresh_session (retry)

8. ALL WORKERS complete
   └─► All tasks done!
```

## 8. Error Handling & Recovery

```
┌─────────────────────────────────────────────────────────────────┐
│                    ERROR SCENARIOS                              │
└─────────────────────────────────────────────────────────────────┘

1. Worker spawn fails:
   ├─► Log error, notify user
   ├─► Continue with other workers
   └─► Manual retry possible

2. Worker gets stuck (5+ same stop reasons):
   ├─► loop_mode = Stalled
   ├─► Notification sent
   └─► User intervention required

3. Worker directory already exists:
   ├─► fs::create_dir_all() succeeds (idempotent)
   ├─► Files overwritten (fresh start)
   └─► Previous state cleared

4. REHOBOAM_LOOP_DIR points to invalid path:
   ├─► find_rehoboam_dir() falls back to search
   ├─► May find wrong directory
   └─► Worker might use Planner's state (BUG)

5. Planner directory missing during worker spawn:
   ├─► anchor.md/guardrails.md copy fails
   ├─► spawn_worker() returns Err
   └─► Worker not created
```
