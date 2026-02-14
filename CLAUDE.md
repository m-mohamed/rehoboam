# CLAUDE.md

Project instructions for Claude Code.

## Overview

Rehoboam is a real-time TUI for monitoring Claude Code agents. Single Rust binary with:
- **TUI mode**: `rehoboam` - Kanban dashboard for agent status
- **Hook mode**: `rehoboam hook` - Process Claude Code hook events from stdin
- **Init mode**: `rehoboam init` - Install hooks to Claude Code projects

**Note**: Rehoboam is a pure monitoring/observability tool. Agent orchestration is handled by Claude Code's TeammateTool.

## Commands

```bash
# Development
cargo check                    # Type check
cargo build --release          # Release build
cargo test                     # Run tests
cargo clippy                   # Lint

# Installation
cargo install rehoboam              # From crates.io
brew tap m-mohamed/rehoboam && brew install rehoboam  # Homebrew

# Usage
rehoboam                       # Start TUI
rehoboam --debug               # TUI with event log
rehoboam hook                  # Process hook event (stdin JSON)
rehoboam hook -N               # Hook with desktop notification
rehoboam init                  # Install hooks to current project
```

## Architecture

```
src/
  main.rs      # CLI entry, hook handler, TUI runner
  app/         # App state machine, spawn dialog, keyboard handling
  cli.rs       # Clap argument parser
  config.rs    # Constants (MAX_AGENTS, socket path)
  event/       # Event system (socket, keyboard input)
  state/       # Agent state, status tracking, role classification
  ui/          # Ratatui widgets (columns, cards, dialogs)
  sprite/      # Remote VM (Sprites) integration
  tmux.rs      # Tmux pane control (send keys, split panes)
```

## Code Style

- Rust 2021 edition
- Use `eyre::Result` for error handling
- Prefer `tracing` macros over `println!` for logging
- Keep functions focused and small
- Use descriptive variable names

## Key Concepts

### Agent Monitoring
- Role detected from `CLAUDE_CODE_AGENT_TYPE` env var (set by TeammateTool)
- Agent behavior patterns inferred from tool usage (AgentRole enum)
- TUI displays both explicit type and observed behavior
- TeammateTool handles orchestration; Rehoboam monitors and displays

### Sprites
- Remote VMs via Fly.io for distributed agent execution

## Git Standards

### Commits
- Follow [Conventional Commits](https://www.conventionalcommits.org/)
- Subject line max 50 chars
- Types: `feat`, `fix`, `perf`, `docs`, `ci`, `chore`

### Pull Requests
- Do NOT mention "Claude Code" or "Anthropic" in PRs
- Do NOT include "Generated with Claude Code" footer
- Keep descriptions focused on changes, not tooling

## Testing

```bash
cargo test                     # All tests
cargo test state               # Agent state tests
cargo test --release           # Release mode tests
```

## Where to Find Things

- Event handling: `src/event/`
- UI rendering: `src/ui/mod.rs`
- Agent state: `src/state/mod.rs`
- Spawn dialog: `src/app/spawn.rs`
