# Development Guide

Get productive with Rehoboam development in under 5 minutes.

## Quick Start

```bash
# Clone the repository
git clone https://github.com/m-mohamed/rehoboam
cd rehoboam

# Build and run with debug panel
cargo run -- --debug

# In another terminal, install hooks to your project
cargo run -- init
```

## Prerequisites

- **Rust (stable)** (stable)
- **tmux** (for local agent spawning and testing)
- **Git**

## Development with Claude Code

Rehoboam is built by Claude Code users, for Claude Code users. We use a feedback loop where you can watch yourself develop in real-time.

### The Feedback Loop

```
┌─────────────────────────────────────────────────────────────┐
│  Terminal 1: Rehoboam                                       │
│  $ cargo run -- --debug                                     │
│  ┌─────────────────────────────────────────────────────────┐│
│  │ Attention │ Working │ Compacting │ Idle                 ││
│  │           │  [You]  │            │                      ││
│  │           │   🤖    │            │                      ││
│  └─────────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│  Terminal 2: Claude Code                                    │
│  $ claude                                                   │
│  > Help me fix the bug in spawn.rs                         │
│  [Your session appears in Terminal 1's dashboard]           │
└─────────────────────────────────────────────────────────────┘
```

### Recommended Setup

1. **Terminal 1** - Run Rehoboam:
   ```bash
   cargo run -- --debug
   ```

2. **Terminal 2** - Start Claude Code:
   ```bash
   claude
   ```

3. Watch your own agent appear in the dashboard as you work

### Testing Features in Real-Time

- **Approve/Reject**: Press `y`/`n` in Rehoboam to approve/reject your own permission requests
- **Spawn agents**: Press `s` to spawn additional agents
- **View modes**: Press `P` to cycle through Kanban/Project/Split views

## Pre-commit Checklist

Run these before pushing:

```bash
cargo fmt --all -- --check      # Format check
cargo clippy --all-targets --all-features -- -D warnings  # Lints
cargo test                      # Unit tests
cargo build --release           # Release build
```

Or run them all:

```bash
cargo fmt --all -- --check && \
cargo clippy --all-targets --all-features -- -D warnings && \
cargo test && \
cargo build --release
```

## Useful Commands

| Command | Description |
|---------|-------------|
| `cargo run -- --debug` | TUI with event log panel |
| `cargo run -- hook` | Process hook event (for testing) |
| `cargo run -- init` | Install hooks to current project |
| `cargo run -- init --all` | Install hooks to all known projects |
| `cargo test` | Run unit tests |
| `cargo doc --open` | Open generated documentation |

## Project Structure

See [ARCHITECTURE.md](ARCHITECTURE.md) for the full code map. Key directories:

```
src/
├── app/      # TUI state machine, keyboard handlers
├── state/    # Agent state management
├── ui/       # Ratatui widgets
├── event/    # Hook events, socket listener
└── rehoboam_loop.rs  # Loop mode logic
```

## Testing Hook Events

You can manually test hook processing:

```bash
# Simulate a hook event
echo '{"session_id":"test","hook_event_name":"UserPromptSubmit","cwd":"/tmp"}' | cargo run -- hook
```

## Debug Logging

Enable tracing for detailed logs:

```bash
RUST_LOG=debug cargo run -- --debug
```

## Release Process (Automated)

Releases are fully automated. **No manual steps required.**

```
Feature PR → Merge to main → release-plz creates Release PR → Merge → Ship
```

**How it works:**
1. Merge your PR to main
2. [release-plz](https://release-plz.ieni.dev/) automatically creates a Release PR with:
   - Version bump (based on conventional commits)
   - Updated CHANGELOG.md (via git-cliff)
3. Review and merge the Release PR
4. [cargo-dist](https://opensource.axo.dev/cargo-dist/) builds binaries and creates GitHub Release

**For local development:** Always run from source with `cargo run --` to stay current.

## Common Issues

### "Socket already in use"

Another Rehoboam instance is running. Kill it:

```bash
pkill rehoboam
```

### Hooks not firing

Re-install hooks:

```bash
cargo run -- init --force
```

### Tests failing on CI

Ensure you're running the full check:

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

## Getting Help

- **Architecture questions**: See [ARCHITECTURE.md](ARCHITECTURE.md)
- **Contribution process**: See [CONTRIBUTING.md](CONTRIBUTING.md)
- **Bug reports**: Open a GitHub issue
