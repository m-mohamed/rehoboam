# Rehoboam

Real-time TUI for monitoring Claude Code agents.

[![Crates.io](https://img.shields.io/crates/v/rehoboam.svg)](https://crates.io/crates/rehoboam)
[![CI](https://github.com/m-mohamed/rehoboam/workflows/CI/badge.svg)](https://github.com/m-mohamed/rehoboam/actions)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

Monitor all your Claude Code sessions from one dashboard. See which agents need attention, which are working, and jump directly to any pane.

## Installation

### Homebrew (macOS/Linux)

```bash
brew tap m-mohamed/rehoboam
brew install rehoboam
```

### Cargo

```bash
cargo install rehoboam
```

### From Source

```bash
cargo install --git https://github.com/m-mohamed/rehoboam
```

## Quick Start

```bash
# Initialize hooks in your project
cd ~/your-project
rehoboam init

# Run
rehoboam
```

## Keybindings

| Key | Action |
|-----|--------|
| `h/l` | Navigate columns |
| `j/k` | Navigate agents |
| `Enter` | Jump to agent's tmux pane |
| `y/n` | Approve/reject permission |
| `c` | Custom input to agent |
| `s` | Spawn new agent |
| `Space` | Toggle selection |
| `Y/N` | Bulk approve/reject |
| `K` | Kill selected agents |
| `X/R` | Cancel/restart loop |
| `?` | Help |
| `q` | Quit |

## Ralph Loops

Autonomous iteration with fresh sessions per loop. Progress persists, failures evaporate.

In spawn dialog (`n`):
- Enable **Loop Mode**
- Set max iterations and stop word
- Rehoboam creates `.ralph/` directory with state files
- Each iteration spawns a fresh Claude session
- Loop stops on stop word detection or max iterations

State files in `.ralph/`:
- `anchor.md` - Task spec (your prompt)
- `progress.md` - Track completed work
- `guardrails.md` - Learned constraints
- `state.json` - Iteration counter

## Sprites (Remote VMs)

Spawn Claude Code agents in isolated cloud VMs via [sprites.dev](https://sprites.dev).

```bash
export SPRITES_API_TOKEN="your-token"
```

| Key | Action |
|-----|--------|
| `Space` | Toggle sprite mode (in spawn dialog) |
| `c` | Create checkpoint |
| `R` | Restore checkpoint |
| `t` | Checkpoint timeline |

## How It Works

```
Claude Code → hooks → Unix socket → Rehoboam TUI
```

Hooks are configured in `.claude/settings.json`. Run `rehoboam init` to set up.

## License

MIT
