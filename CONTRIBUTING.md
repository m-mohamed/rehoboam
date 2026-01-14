# Contributing to Rehoboam

Thanks for your interest in contributing! This document outlines the process and standards for contributing to Rehoboam.

**Quick links:**
- [ARCHITECTURE.md](ARCHITECTURE.md) - Codebase map and design decisions
- [DEVELOPMENT.md](DEVELOPMENT.md) - Local development setup

**Standards we follow:**
- [Conventional Commits](https://www.conventionalcommits.org/) - Commit message format
- [Semantic Versioning](https://semver.org/) - Version numbering
- [Keep a Changelog](https://keepachangelog.com/) - Changelog format
- [CLIG](https://clig.dev/) - CLI design guidelines

## Prerequisites

- Rust 1.85+ (stable)
- tmux (for local agent testing)
- Git

## Development Setup

```bash
# Clone the repository
git clone https://github.com/m-mohamed/rehoboam.git
cd rehoboam

# Build
cargo build

# Run tests
cargo test

# Run with debug logging
cargo run -- --debug
```

## Developing with Claude Code

Rehoboam is built by Claude Code users, for Claude Code users. We encourage using Claude Code to contribute.

### The Feedback Loop

When developing Rehoboam with Claude Code, you can watch yourself work in real-time:

1. **Terminal 1** - Run Rehoboam:
   ```bash
   cargo run -- --debug
   ```

2. **Terminal 2** - Start Claude Code:
   ```bash
   claude
   ```

3. Your agent appears in the dashboard as you develop
4. Test approve/reject (`y`/`n`), spawn (`s`), and other features live

### Claude Code Hooks

Rehoboam receives events via hooks. After `rehoboam init`:
- Your `.claude/settings.json` has hooks configured
- Every Claude Code action sends events to Rehoboam

See [DEVELOPMENT.md](DEVELOPMENT.md) for the full development workflow.

## Code Quality Standards

### Formatting

All code must be formatted with `rustfmt`:

```bash
cargo fmt --all
```

### Linting

All code must pass `clippy` with no warnings:

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

### Testing

Run the full test suite before submitting:

```bash
cargo test --all-features
```

### Pre-commit Checklist

```bash
cargo fmt --all -- --check  # Formatting
cargo clippy --all-targets --all-features -- -D warnings  # Lints
cargo test --all-features   # Tests
cargo build --release       # Release build
```

## Architecture

```
src/
  main.rs      # CLI entry, hook handler, TUI runner
  app/         # App state machine (split into focused modules)
    mod.rs     # Core App struct, event handling
    keyboard.rs    # Keyboard input handlers
    spawn.rs       # Agent spawning logic
    operations.rs  # Git operations, diff view
    agent_control.rs  # Approve/reject/kill actions
    navigation.rs     # Pane jumping, search
  cli.rs       # Clap argument parser
  config.rs    # Constants (MAX_AGENTS, socket path)
  event/       # Event system (socket, keyboard input)
  state/       # Agent state, status tracking
  ui/          # Ratatui widgets (columns, cards)
  tui.rs       # Terminal setup/restore
  init.rs      # Hook installer
  notify.rs    # Desktop notifications
```

## Commit Standards

We follow [Conventional Commits](https://www.conventionalcommits.org/):

### Format

```
<type>: <subject>

<body>
```

### Types

- `feat` - New feature
- `fix` - Bug fix
- `perf` - Performance improvement
- `docs` - Documentation only
- `ci` - CI/CD changes
- `chore` - Maintenance, refactoring

### Rules

- Subject line max 50 characters
- Use imperative mood ("add" not "added")
- No period at the end of subject
- Body explains *why*, not *what*

### Examples

```
feat: add Kanban column layout

- 4 status columns with card navigation
- h/l for columns, j/k for cards
```

```
fix: prevent panic on empty agent list

Check bounds before indexing into agents vector.
```

## Branch Naming

Use descriptive branch names with these prefixes:

| Prefix | Purpose | Example |
|--------|---------|---------|
| `feat/` | New features | `feat/loop-mode` |
| `fix/` | Bug fixes | `fix/panic-empty-list` |
| `chore/` | Maintenance | `chore/update-deps` |
| `docs/` | Documentation | `docs/api-reference` |
| `ci/` | CI/CD changes | `ci/add-msrv-check` |
| `perf/` | Performance | `perf/reduce-allocations` |

## Pull Request Process

1. **Branch from main**: Create a feature branch
   ```bash
   git checkout -b feat/my-feature
   ```

2. **Make changes**: Follow the code quality standards above

3. **Test locally**: Run the pre-commit checklist

4. **Push and open PR**: Include a clear description of changes

5. **AI Code Review**: Mention `@greptile` in a PR comment to trigger an AI-assisted code review

6. **Address feedback**: Respond to review comments

## Rust Best Practices

### Error Handling

- Use `Result<T, E>` for fallible operations
- Prefer `.expect("reason")` over `.unwrap()` in non-test code
- Use the `?` operator for error propagation

### Memory & Performance

- Avoid unnecessary allocations in hot paths
- Use `&str` over `String` for function parameters when possible
- Consider `Cow<str>` for conditionally owned strings

### Type Safety

- Use `#[must_use]` on functions whose return values shouldn't be ignored
- Prefer newtypes over raw primitives for domain concepts
- Use enums for state machines

### Documentation

- Document public APIs with `///` doc comments
- Include examples in doc comments for complex functions
- Keep comments focused on *why*, not *what*

## Releases and Versioning

We follow [Semantic Versioning](https://semver.org/):

- **MAJOR** (`1.0.0`): Breaking changes
- **MINOR** (`0.2.0`): New features, backwards compatible
- **PATCH** (`0.1.1`): Bug fixes, backwards compatible

### Release Process

Releases are fully automated via [cargo-dist](https://opensource.axo.dev/cargo-dist/). Push a tag and everything deploys.

1. **Update version** in `Cargo.toml`
2. **Update CHANGELOG.md** with release notes
3. **Commit and tag**:
   ```bash
   git commit -m "chore: bump to v0.x.y"
   git tag v0.x.y
   git push && git push --tags
   ```

CI automatically:
- Builds binaries for 4 platforms (macOS Intel/ARM, Linux Intel/ARM)
- Creates GitHub Release with downloadable binaries
- Updates Homebrew tap (`brew upgrade rehoboam`)
- Generates shell installer script

No manual steps required after tagging.

### Changelog Format

```markdown
## [0.2.0] - 2025-01-15

### Added
- Loop mode for running agents repeatedly

### Changed
- Improved status detection

### Fixed
- Panic on empty agent list
```

## Getting Help

- Open an issue for bugs or feature requests
- Check existing issues before creating new ones

## License

By contributing, you agree that your contributions will be licensed under the MIT License.
