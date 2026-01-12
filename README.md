# Rehoboam

Real-time TUI for monitoring Claude Code agents.

[![CI](https://github.com/m-mohamed/rehoboam/workflows/CI/badge.svg)](https://github.com/m-mohamed/rehoboam/actions)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.75+-orange.svg)](https://www.rust-lang.org)

Monitor all your Claude Code sessions from a single dashboard. See which agents need attention, which are working, and jump directly to any pane.

## Why Rehoboam?

When running multiple Claude Code agents across projects, you lose track of:

- Which agents need your attention (permission prompts)
- Which are actively working vs. idle
- What tools they're using right now
- When context compaction is happening

Rehoboam gives you a unified dashboard with desktop notifications when attention is needed.

## Features

**Core Monitoring**
- Kanban-style dashboard (Attention, Working, Compacting, Idle columns)
- 5-20ms latency via Unix socket
- Desktop notifications on permission requests
- Jump to any agent's pane with Enter

**Agent Control**
- Spawn new agents from the TUI (`n` key)
- Loop mode for autonomous iteration (Ralph-style)
- Subagent tracking and visualization

**Sprites Integration** (Remote VMs)
- Spawn agents in isolated cloud VMs via [sprites.dev](https://sprites.dev)
- Clone any GitHub repo into a Sprite
- Checkpoint/restore for instant VM snapshots
- Network policy presets (Full, Claude-only, Restricted)

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

## Quick Start

**1. Initialize hooks in your project:**

```bash
cd ~/your-project
rehoboam init
```

**2. Start the TUI:**

```bash
rehoboam
```

That's it. Claude Code hooks will send events to Rehoboam via Unix socket.

## Usage

```bash
rehoboam              # Start TUI
rehoboam --debug      # Start with debug log visible
rehoboam init         # Install hooks to current project
rehoboam init --all   # Install hooks to multiple projects interactively
```

## Keybindings

### Navigation

| Key | Action |
|-----|--------|
| `h` / `l` | Navigate columns |
| `j` / `k` | Navigate cards |
| `Enter` | Jump to agent pane |
| `?` | Show help |
| `q` | Quit |

### Agent Control

| Key | Action |
|-----|--------|
| `n` | Spawn new agent |
| `y` / `Y` | Send "y" (approve permission) |
| `N` | Send "n" (deny permission) |
| `K` | Kill agent |

### Display

| Key | Action |
|-----|--------|
| `d` | Toggle debug log |
| `f` | Freeze display |
| `g` | Toggle git diff viewer |
| `p` | Toggle diff preview |
| `s` | Toggle subagent panel |

### Sprites (Remote VMs)

| Key | Action |
|-----|--------|
| `Space` | Toggle sprite mode (in spawn dialog) |
| `c` | Create checkpoint |
| `R` | Restore checkpoint |
| `t` | Toggle checkpoint timeline |
| `P` | Cycle network policy |
| `x` | Destroy sprite |

## Status Indicators

| Status | Icon | Meaning |
|--------|------|---------|
| Working | `[W]` | Agent is processing |
| Attention | `[A]` | Needs user input (permission) |
| Compacting | `[C]` | Context compaction in progress |
| Idle | `[I]` | Waiting for commands |

## Sprites Integration

Rehoboam integrates with [sprites.dev](https://sprites.dev) for running Claude Code in isolated cloud VMs.

**Why Sprites?**
- Isolated environments for risky operations
- Instant checkpoints (~300ms snapshots)
- Pre-installed Claude Code + GitHub CLI
- Network sandboxing

**Setup:**

```bash
export SPRITES_API_TOKEN="your-token"
```

**Spawn a Sprite agent:**

1. Press `n` to open spawn dialog
2. Press `Space` to enable sprite mode
3. Enter GitHub repo (e.g., `owner/repo` or full URL)
4. Enter your prompt
5. Press `Enter` to spawn

**Network Presets:**

| Preset | Access |
|--------|--------|
| Full | Unrestricted internet |
| Claude Only | API + GitHub + package registries |
| Restricted | No external network |

## Loop Mode

Loop mode enables Ralph-style autonomous iteration. When enabled, Rehoboam automatically sends Enter after each Claude response, creating a continuous work loop.

**Safeguards:**
- Max iteration limit
- Stop word detection
- Stall detection (5+ identical responses)

## Terminal Support

| Terminal | Pane ID | Jump to Pane |
|----------|---------|--------------|
| **Tmux** | `TMUX_PANE` | Full support |
| **Ghostty** | `GHOSTTY_RESOURCES_DIR` | Via tmux |
| **WezTerm** | `WEZTERM_PANE` | Full support |
| **Kitty** | `KITTY_WINDOW_ID` | Planned |
| **iTerm2** | `ITERM_SESSION_ID` | Planned |
| **Other** | session_id fallback | Navigation only |

## How It Works

```
Claude Code → hooks → rehoboam hook → Unix socket → rehoboam TUI
```

Claude Code triggers hook events (PreToolUse, PermissionRequest, Stop, etc.). The hook command sends JSON to `/tmp/rehoboam.sock`. The TUI receives events asynchronously and updates the display.

## Configuration

Hooks are configured in each project's `.claude/settings.json`. Run `rehoboam init` to set up automatically.

**Socket path:** `/tmp/rehoboam.sock` (or `$XDG_RUNTIME_DIR/rehoboam.sock` on Linux)

**Sprites config:** Set `SPRITES_API_TOKEN` environment variable

## Troubleshooting

**Hooks not firing**
- Verify `.claude/settings.json` exists in your project
- Restart Claude Code (hooks load at session start)

**Socket connection failed**
- Check for existing instance: `lsof /tmp/rehoboam.sock`
- Remove stale socket: `rm /tmp/rehoboam.sock`

**Pane jumping doesn't work**
- Tmux: Ensure `$TMUX_PANE` is set (run `echo $TMUX_PANE`)
- WezTerm: Ensure `$WEZTERM_PANE` is set

**Agent stuck in Working**
- Agents auto-transition to Idle after 60s of inactivity (when not in an active response)
- Stale sessions removed after 5 minutes

## Requirements

- **Rust 1.75+** for building
- **Claude Code** with hooks configured
- **Tmux** or **WezTerm** recommended for pane jumping
- **Sprites API token** (optional, for remote VMs)

## Limitations

- **Jump to pane**: Tmux and WezTerm only
- **Local only**: Unix socket, no remote monitoring
- **macOS/Linux**: No Windows support

## License

MIT
