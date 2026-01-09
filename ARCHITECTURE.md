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
| Missing terminal env var | Falls back to session_id | Agent still tracked |
| SessionEnd never sent | Agent persists until TUI restart |

## Testing

```bash
cargo test           # Unit tests (init.rs, event parsing)
cargo clippy         # Lints
cargo fmt --check    # Formatting
```

Integration testing: Run `rehoboam` and trigger hooks manually with `rehoboam hook` subcommand.
