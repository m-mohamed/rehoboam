# rehoboam

Real-time TUI for monitoring Claude Code agents.

[![CI](https://github.com/m-mohamed/rehoboam/workflows/CI/badge.svg)](https://github.com/m-mohamed/rehoboam/actions)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

<!-- TODO: Add demo GIF here -->
<!-- ![rehoboam demo](demo.gif) -->

## Features

- Kanban-style dashboard with status columns (Attention, Working, Compacting, Idle)
- 5-20ms end-to-end latency via Unix socket
- Desktop notifications on permission requests (`-N` flag)
- Jump to agent pane with Enter key

## Installation

```bash
cargo install --git https://github.com/m-mohamed/rehoboam
```

Or build from source:

```bash
git clone https://github.com/m-mohamed/rehoboam
cd rehoboam
cargo build --release
cp target/release/rehoboam ~/.local/bin/
```

## Requirements

- **Claude Code** - With hooks configured (see Quick Start)
- **Recommended**: WezTerm, Kitty, or iTerm2 for pane identification
- Works with any terminal (uses session_id fallback)

## Quick Start

1. Initialize hooks in your project:
   ```bash
   rehoboam init
   ```

2. Start the TUI:
   ```bash
   rehoboam
   ```

That's it. Claude Code hooks will send events to rehoboam via Unix socket.

## Usage

```
rehoboam              # Start TUI
rehoboam --debug      # Start with event log visible
rehoboam init         # Install hooks to current project
rehoboam init --all   # Install hooks to multiple projects
```

### Keybindings

| Key | Action |
|-----|--------|
| `h`/`l` | Navigate columns |
| `j`/`k` | Navigate cards within column |
| `Enter` | Jump to agent pane |
| `f` | Freeze display |
| `d` | Toggle debug mode |
| `?` | Show help |
| `q` | Quit |

### Status Indicators

| Status | Icon | Meaning |
|--------|------|---------|
| Working | 🤖 | Agent is processing |
| Attention | 🔔 | Needs user input (permission request) |
| Compacting | 🔄 | Context compaction in progress |
| Idle | ⏸️ | Waiting for commands |

## How It Works

```
Claude Code → hooks → rehoboam hook → Unix socket → rehoboam TUI
```

Claude Code triggers hook events (PreToolUse, PermissionRequest, etc.). The hook command sends JSON to `/tmp/rehoboam.sock`. The TUI receives events via tokio async and updates the display.

See [ARCHITECTURE.md](ARCHITECTURE.md) for implementation details.

## Limitations

- **Jump to pane** (`Enter` key): WezTerm only
- **Local only** - Unix socket at `/tmp/rehoboam.sock`. No remote monitoring.
- **macOS/Linux** - No Windows support (Unix sockets).

## License

MIT
