# CLAUDE.md

Project instructions for Claude Code.

## Overview

Rehoboam is a real-time TUI for monitoring Claude Code agents. Single Rust binary with:
- **TUI mode**: `rehoboam` - Kanban dashboard for agent status
- **Hook mode**: `rehoboam hook` - Process Claude Code hook events from stdin

## Commands

```bash
# Development
cargo check                    # Type check
cargo build --release          # Release build
cargo test                     # Run tests
cargo clippy                   # Lint

# Installation
cargo build --release && cp target/release/rehoboam ~/.local/bin/

# Usage
rehoboam                       # Start TUI
rehoboam --debug               # TUI with event log
rehoboam hook                  # Process hook event (stdin JSON)
rehoboam hook -N               # Hook with desktop notification
rehoboam init                  # Install hooks to current project
rehoboam init --all            # Install hooks to all known projects
```

## Architecture

```
src/
  main.rs      # CLI entry, hook handler, TUI runner
  app.rs       # App state machine
  cli.rs       # Clap argument parser
  config.rs    # Constants (MAX_AGENTS, socket path)
  event/       # Event system (socket, keyboard input)
  state/       # Agent state, status tracking
  ui/          # Ratatui widgets (columns, cards)
  tui.rs       # Terminal setup/restore
  init.rs      # Hook installer
  notify.rs    # Desktop notifications
```

## Git Commit Standards

**Local**: Work-in-progress commits can be messy.

**Before pushing to main**: Must be clean.

### Rules
- Follow [Conventional Commits](https://www.conventionalcommits.org/)
- Subject line max 50 chars
- No version numbers in scope
- No implementation details in message
- Squash WIP commits before push

### Types
- `feat`: New feature
- `fix`: Bug fix
- `perf`: Performance improvement
- `docs`: Documentation
- `ci`: CI/CD changes
- `chore`: Maintenance

### Example
```
feat: add Kanban column layout

- 4 status columns with card navigation
- h/l for columns, j/k for cards
```
