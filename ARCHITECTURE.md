# Architecture

This document describes rehoboam's high-level architecture. For implementation details, see inline rustdoc comments.

## Bird's Eye View

Claude Code hooks → Unix socket → async event loop → ratatui TUI

Rehoboam monitors Claude Code agents by receiving hook events via Unix socket and displaying status in a terminal UI.

## Code Map

```
src/
├── main.rs           Entry point, CLI parsing, hook command, TUI event loop
├── cli.rs            Clap argument definitions
├── config.rs         Constants (timeouts, limits, colors)
├── app.rs            Application state wrapper, keyboard handling
├── init.rs           Hook installation to .claude/settings.json
├── notify.rs         Desktop notification support (-N flag)
├── event/
│   ├── mod.rs        HookEvent struct, ClaudeHookInput parsing, derive_status()
│   ├── socket.rs     Unix socket listener (tokio), connection handling
│   └── input.rs      Keyboard event stream
├── state/
│   ├── mod.rs        AppState, process_event(), agents_by_column()
│   └── agent.rs      Agent struct, Status enum, tool latency tracking
└── ui/
    ├── mod.rs        Main render function, layout
    ├── column.rs     Status column rendering
    └── card.rs       Agent card rendering
```

## Data Flow

```
┌──────────────┐     ┌──────────────┐     ┌──────────────┐     ┌──────────────┐
│ Claude Code  │     │ rehoboam     │     │ Socket       │     │ TUI          │
│ Hook         │────▶│ hook cmd     │────▶│ Listener     │────▶│ Render       │
│ (subprocess) │     │ (stdin→sock) │     │ (tokio)      │     │ (ratatui)    │
└──────────────┘     └──────────────┘     └──────────────┘     └──────────────┘
     1-5ms               1-2ms                 1ms                 2-10ms
```

**End-to-end latency**: 5-20ms typical (well under 100ms human perception threshold)

## Hook Event Schema

Claude Code sends JSON via stdin to `rehoboam hook`. The `ClaudeHookInput` struct parses these fields:

### Required Fields

| Field | Type | Description |
|-------|------|-------------|
| `session_id` | string | Unique session identifier |
| `hook_event_name` | string | Event type (see Status Derivation) |
| `transcript_path` | string | Path to .jsonl conversation file |
| `cwd` | string | Current working directory |

### Optional Fields

| Field | Type | Present In | Description |
|-------|------|------------|-------------|
| `tool_name` | string | PreToolUse, PostToolUse | Tool being executed |
| `tool_input` | object | PreToolUse, PostToolUse | Tool parameters |
| `tool_use_id` | string | PreToolUse, PostToolUse | Correlates Pre→Post for latency |
| `tool_response` | object | PostToolUse | Tool result |
| `permission_mode` | string | All | Current permission mode |
| `user_prompt` | string | UserPromptSubmit | The user's prompt text |
| `reason` | string | Stop, SessionEnd | Why stopping |
| `trigger` | string | PreCompact | "manual" or "auto" |
| `source` | string | SessionStart | "startup", "resume", "clear", "compact" |
| `message` | string | Notification | Notification message |

### Status Derivation

The `derive_status()` function maps hook events to TUI status:

| Hook Event | → Status | Attention Type |
|------------|----------|----------------|
| SessionStart | idle | - |
| UserPromptSubmit | working | - |
| PreToolUse | working | - |
| PostToolUse | working | - |
| PermissionRequest | attention | permission |
| Notification | attention | notification |
| Stop | idle | - |
| SessionEnd | idle (removes agent) | - |
| PreCompact | compacting | - |
| SubagentStop | working | - |
| (unknown) | idle | - |

## Invariants

- **UI is read-only**: Never writes to socket or modifies external state
- **State is single source of truth**: All rendering reads from AppState
- **Events processed in order**: mpsc channel preserves FIFO ordering
- **Bounded memory**: Max 500 agents (LRU eviction), 60 sparkline points, 50 event log entries
- **Non-blocking hooks**: 500ms timeout ensures Claude Code never waits

## Design Decisions

### Why Unix socket?

Fastest local IPC option. No serialization overhead (just JSON lines). Fire-and-forget pattern - hooks don't wait for response.

Alternatives considered:
- **Named pipe**: Similar performance, but less portable
- **TCP localhost**: Works but unnecessary network stack overhead
- **Shared memory**: Fastest, but complexity not worth ~0.5ms savings

### Why tokio?

Async runtime matches ratatui's event loop pattern. `tokio::select!` handles both socket events and keyboard input cleanly. Single-threaded executor avoids lock contention.

### Why 500ms hook timeout?

Hooks must never block Claude Code. 500ms is long enough for any reasonable socket operation, short enough that users won't notice if TUI is down.

### Why cached status counts?

Header displays agent counts by status. Before v1.2.0, this iterated all agents 4 times per frame (O(4n)). Now counts are updated incrementally in `process_event()` for O(1) lookup.

### Why Kanban columns?

Users care about one question: "Does any agent need my attention?" Four columns (Attention, Working, Compacting, Idle) answer this at a glance. Attention column on the left draws the eye first.

## Performance

### Latency by Stage

| Stage | Typical | Timeout |
|-------|---------|---------|
| Hook subprocess spawn | 1-5ms | - |
| Hook stdin→socket | 1-2ms | 500ms |
| Socket read + parse | 1ms | 2s |
| State update | 0.1ms | - |
| Render | 2-10ms | - |

### Complexity

| Operation | Complexity |
|-----------|------------|
| Agent lookup | O(1) HashMap |
| Status counts | O(1) cached |
| Column grouping | O(n log k) |
| Sparkline update | O(1) VecDeque push |

### Limits

| Resource | Limit | Rationale |
|----------|-------|-----------|
| Max agents | 500 | Memory bound, LRU eviction |
| Max connections | 100 | Prevent resource exhaustion |
| Sparkline points | 60 | ~1 minute of history |
| Event log | 50 | Debug visibility |
| Channel capacity | 100 | Backpressure buffer |

## Failure Modes

| Failure | Behavior |
|---------|----------|
| TUI not running | Hooks exit silently, Claude continues |
| Socket full | New connections dropped, recovers automatically |
| Invalid JSON | Logged and skipped |
| Missing terminal env var | Falls back to session_id (agent still tracked) |
| SessionEnd never sent | Agent cleaned up after 5 min inactivity |

## Terminal Support

### Pane Identification Priority

The hook command checks environment variables in this order:
1. `WEZTERM_PANE` - WezTerm pane ID (numeric)
2. `TMUX_PANE` - Tmux pane ID (`%0`, `%1`, etc.)
3. `KITTY_WINDOW_ID` - Kitty terminal
4. `ITERM_SESSION_ID` - iTerm2 session
5. `session_id[0:8]` - Fallback (first 8 chars of Claude session ID)

### Jump-to-Pane Support

| Terminal | Pane ID Format | CLI Command |
|----------|----------------|-------------|
| Tmux | `%0`, `%1` | `tmux select-pane -t %N` |
| WezTerm | numeric | `wezterm cli activate-pane --pane-id N` |
| Kitty | numeric | Not yet implemented |
| iTerm2 | string | Not yet implemented |

## Testing

```bash
cargo test           # Unit tests (init.rs, event parsing)
cargo clippy         # Lints
cargo fmt --check    # Formatting
```

Integration testing: Run `rehoboam` and trigger hooks manually with `rehoboam hook` subcommand.
