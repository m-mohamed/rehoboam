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

- **Rust 1.70+** (stable)
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
└── ralph.rs  # Loop mode logic
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

## Release Process (Maintainers)

We use cargo-dist for fully automated releases:

1. **Update version** in `Cargo.toml`
2. **Update** `CHANGELOG.md`
3. **Commit**: `git commit -m "chore: bump to v0.x.y"`
4. **Tag**: `git tag v0.x.y`
5. **Push**: `git push && git push --tags`

CI automatically:
- Builds binaries for macOS (Intel + ARM) and Linux (Intel + ARM)
- Creates GitHub Release with downloadable binaries
- Updates Homebrew tap (`brew upgrade rehoboam`)
- Generates shell installer

No manual steps required after tagging.

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
