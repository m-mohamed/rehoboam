---
description: Set up the recommended two-terminal development feedback loop for Rehoboam
allowed-tools: Bash(cargo:*), Bash(tmux:*), Bash(which:*), Read
---

# Development Loop Setup

Set up the recommended Rehoboam development workflow with real-time TUI monitoring.

## Prerequisites Check

1. Verify tmux is installed:
   ```bash
   which tmux
   ```

2. Check if Rehoboam is already running (port/socket conflict check)

## Setup Options

Offer the user these choices:

### Option 1: Split Current Tmux Window (if in tmux)
```bash
# Split horizontally
tmux split-window -h

# Left pane: Run Rehoboam TUI with debug logging
cargo run -- --debug

# Right pane: Ready for claude commands
```

### Option 2: Create New Tmux Session
```bash
# Create new session named 'rehoboam-dev'
tmux new-session -d -s rehoboam-dev

# First window for TUI
tmux send-keys -t rehoboam-dev 'cargo run -- --debug' Enter

# Split for Claude work
tmux split-window -h -t rehoboam-dev

# Attach to session
tmux attach -t rehoboam-dev
```

### Option 3: Manual Instructions
Provide step-by-step instructions for users not using tmux:
- Terminal 1: `cargo run -- --debug`
- Terminal 2: Run `claude` sessions

## Post-Setup

After setup, explain:
1. The left pane shows real-time agent status
2. The right pane is for running Claude Code sessions
3. Press `q` to quit the TUI
4. Use `--debug` flag to see event logs
