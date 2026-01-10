# rehoboam

Real-time TUI for monitoring Claude Code agents.

[![CI](https://github.com/m-mohamed/rehoboam/workflows/CI/badge.svg)](https://github.com/m-mohamed/rehoboam/actions)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

![Rehoboam Kanban Dashboard](docs/screenshot.png)

## Why Rehoboam?

When running multiple Claude Code agents across projects, it's hard to know:
- Which agents need your attention (permission prompts)
- Which are actively working vs. idle
- When context compaction is happening

Rehoboam gives you a single dashboard to monitor all agents in real-time, with desktop notifications when attention is needed.

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
- **Recommended**: Tmux, WezTerm, Kitty, or iTerm2 for pane identification
- Works with any terminal (uses session_id fallback)

## Terminal Support

| Terminal | Pane ID | Jump to Pane |
|----------|---------|--------------|
| **Tmux** | `TMUX_PANE` (%0, %1) | `tmux select-pane` |
| **WezTerm** | `WEZTERM_PANE` | `wezterm cli activate-pane` |
| **Kitty** | `KITTY_WINDOW_ID` | Not yet |
| **iTerm2** | `ITERM_SESSION_ID` | Not yet |
| **Other** | session_id fallback | Not supported |

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

## Configuration

Hooks are configured in each project's `.claude/settings.json`. Run `rehoboam init` to install or update hooks automatically.

Default socket path: `/tmp/rehoboam.sock` (or `$XDG_RUNTIME_DIR/rehoboam.sock` on Linux)

See [ARCHITECTURE.md](ARCHITECTURE.md) for hook event details.

## Troubleshooting

### Hooks not firing
- Verify hooks are installed: check `.claude/settings.json` exists in your project
- Restart Claude Code to pick up new hooks (hooks load at session start)

### Socket connection failed
- Check if another rehoboam instance is running: `lsof /tmp/rehoboam.sock`
- Remove stale socket: `rm /tmp/rehoboam.sock`

### Pane jumping doesn't work
- **Tmux**: Ensure you're inside a tmux session (`echo $TMUX_PANE` should show `%0`, `%1`, etc.)
- **WezTerm**: Ensure `WEZTERM_PANE` environment variable is set
- Other terminals: Jump-to-pane not yet supported (card navigation still works)

### Agent stuck in Working
- Agents auto-transition to Idle after 30s of inactivity
- Stale sessions are removed after 5 minutes

## Limitations

- **Jump to pane** (`Enter` key): Tmux and WezTerm only (Kitty, iTerm2 planned)
- **Local only** - Unix socket at `/tmp/rehoboam.sock`. No remote monitoring.
- **macOS/Linux** - No Windows support (Unix sockets).

## License

MIT
