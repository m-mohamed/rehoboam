# CLAUDE.md

Project instructions for Claude Code.

## Overview

Rehoboam is a real-time TUI for monitoring Claude Code agents. Single Rust binary with:
- **TUI mode**: `rehoboam` - Kanban dashboard for agent status
- **Hook mode**: `rehoboam hook` - Process Claude Code hook events from stdin
- **Loop mode**: Rehoboam's Loop autonomous iterations with `.rehoboam/` state

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
rehoboam spawn --loop          # Spawn agent with loop mode
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
  rehoboam_loop.rs  # Loop state management, task queue, role prompts
  sprite/      # Remote VM (Sprites) integration
  git.rs       # Git worktree and checkpoint support
```

## Code Style

- Rust 2021 edition
- Use `eyre::Result` for error handling
- Prefer `tracing` macros over `println!` for logging
- Keep functions focused and small
- Use descriptive variable names

## Key Concepts

### Loop Mode (Rehoboam's Loop)
- `.rehoboam/` directory stores loop state
- `anchor.md` = immutable task spec
- `progress.md` = work completed each iteration
- `tasks.md` = task queue for Planner/Worker separation
- `guardrails.md` = learned constraints

### Agent Roles (Cursor-aligned)
- **Planner**: Explores codebase, creates tasks in `tasks.md`
- **Worker**: Executes single task in isolation
- **Auto**: Legacy generic prompt behavior

### Sprites
- Remote VMs via Fly.io for distributed agent execution
- Checkpoint/restore support for state persistence

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
cargo test rehoboam_loop       # Loop mode tests
cargo test state               # Agent state tests
cargo test --release           # Release mode tests
```

## Where to Find Things

- Event handling: `src/event/`
- UI rendering: `src/ui/mod.rs`
- Agent state: `src/state/mod.rs`
- Loop logic: `src/rehoboam_loop.rs`
- Spawn dialog: `src/app/spawn.rs`
