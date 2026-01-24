# Rehoboam State Machines

This document describes the state machines and flows in Rehoboam.

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
                    │ YES              │                  │ NO
                    ▼                  │                  ▼
         ┌──────────────────┐          │     ┌──────────────────┐
         │ Judge Evaluation │          │     │ Normal Stop      │
         │                  │          │     │ Processing       │
         │ • Check progress │          │     └──────────────────┘
         │ • Check stall    │          │
         │ • Check max iter │          │
         └────────┬─────────┘          │
                  │                    │
     ┌────────────┼────────────────────┤
     │            │                    │
     ▼            ▼                    ▼
┌─────────┐ ┌─────────┐         ┌──────────┐
│ COMPLETE│ │ STALLED │         │ CONTINUE │
│         │ │         │         │          │
│ Set:    │ │ Set:    │         │ Action:  │
│ Complete│ │ Stalled │         │ respawn  │
│         │ │         │         │ fresh    │
│ Notify  │ │ Notify  │         │ session  │
│ user    │ │ user    │         │          │
└─────────┘ └─────────┘         └──────────┘
```

## 3. Agent Status Flow

```
                              ┌──────────────────┐
                              │  SessionStart    │
                              │  Event           │
                              └────────┬─────────┘
                                       │
                                       ▼
                              ┌──────────────────┐
                              │  Status: Waiting │
                              └────────┬─────────┘
                                       │
              ┌────────────────────────┼────────────────────────┐
              │                        │                        │
              ▼                        ▼                        ▼
     ┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
     │ ToolUse Event   │     │ Permission      │     │ Stop Event      │
     │                 │     │ Request         │     │                 │
     │ Status: Working │     │                 │     │ Status: Waiting │
     └────────┬────────┘     │ Status:         │     │ (if loop:       │
              │              │ Attention       │     │  Continue)      │
              │              └────────┬────────┘     │                 │
              │                       │              │ Status: Done    │
              │                       │              │ (if complete)   │
              └───────────────────────┴──────────────┴─────────────────┘

```

## 4. Hook Event Processing

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
│  3. Capture TeammateTool env vars:                              │
│     • CLAUDE_CODE_TEAM_NAME                                     │
│     • CLAUDE_CODE_AGENT_ID                                      │
│     • CLAUDE_CODE_AGENT_NAME                                    │
│     • CLAUDE_CODE_AGENT_TYPE                                    │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│                 Send to TUI via Unix socket                     │
│                                                                 │
│  HookEvent includes pane_id for routing                         │
│  TUI looks up pending_loop_config by pane_id                    │
│  Applies loop config to agent (loop_dir, etc.)                  │
└─────────────────────────────────────────────────────────────────┘
```

## 5. Full Lifecycle Example

```
1. USER: rehoboam spawn --loop
   └─► Creates .rehoboam/, starts agent

2. AGENT works on task
   └─► Reads anchor.md, updates progress.md

3. AGENT reaches completion
   └─► Stop event triggers

4. TUI: process_event() handles Stop
   ├─► Judge evaluates progress
   ├─► If COMPLETE or STALLED: loop_mode set accordingly
   └─► If CONTINUE: spawn_fresh_session()

5. Loop continues until:
   ├─► Stop word found in progress.md
   ├─► <promise> tag detected
   ├─► Max iterations reached
   ├─► 5+ identical stop reasons (stalled)
   └─► User intervention (X key)
```

## 6. Error Handling & Recovery

```
┌─────────────────────────────────────────────────────────────────┐
│                    ERROR SCENARIOS                              │
└─────────────────────────────────────────────────────────────────┘

1. Agent gets stuck (5+ same stop reasons):
   ├─► loop_mode = Stalled
   ├─► Notification sent
   └─► User intervention required

2. Error pattern detected 3+ times:
   ├─► Auto-added to guardrails.md
   └─► Agent sees warning on next iteration

3. Socket connection fails:
   ├─► Hook logs error
   ├─► Agent continues (hooks are non-blocking)
   └─► TUI misses event

4. Loop directory missing:
   ├─► State load fails
   ├─► Loop iteration skipped
   └─► Error logged
```

## TeammateTool Integration

Rehoboam monitors TeammateTool operations for observability:

```
┌─────────────────────────────────────────────────────────────────┐
│                 TeammateTool Environment                        │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│                 Agent State Fields                              │
│                                                                 │
│  team_name: Option<String>       ← CLAUDE_CODE_TEAM_NAME        │
│  agent_id: Option<String>        ← CLAUDE_CODE_AGENT_ID         │
│  agent_name: Option<String>      ← CLAUDE_CODE_AGENT_NAME       │
│  team_agent_type: Option<String> ← CLAUDE_CODE_AGENT_TYPE       │
│                                                                 │
│  These are display-only, not control mechanisms.                │
│  TeammateTool handles actual orchestration.                     │
└─────────────────────────────────────────────────────────────────┘
```
