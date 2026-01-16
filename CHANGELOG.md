# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.9.4] - 2026-01-16

### Changed

- Enable auto-merge for release-plz PRs

### Fixed

- Use pr_automerge action input instead of invalid config field
[0.9.4]: https://github.com/m-mohamed/rehoboam/compare/v0.9.3...v0.9.4


## [0.9.3] - 2026-01-16

### Added

- Scaling Agents - Hierarchical Role System ([#17](https://github.com/m-mohamed/rehoboam/pull/17))
[0.9.3]: https://github.com/m-mohamed/rehoboam/compare/v0.9.2...v0.9.3


## [0.9.2] - 2026-01-14

### Added

- Enhanced agent spawn with GitHub clone and sprite management ([#9](https://github.com/m-mohamed/rehoboam/pull/9))
- Add tmux-based reconciliation polling ([#10](https://github.com/m-mohamed/rehoboam/pull/10))
- Add dashboard overlay and context-aware footer
- Add split view with live agent output
- Enhance Ralph loops with observability

### Changed

- Reset version to 0.9.1 for release-plz bootstrap
- Bump version to 0.9.3 for release-plz bootstrap
- Add shell completions for release
- Merge Idle into Attention with Waiting type ([#8](https://github.com/m-mohamed/rehoboam/pull/8))
- Add Rust best practices and remove dead code ([#5](https://github.com/m-mohamed/rehoboam/pull/5))
- Split app.rs into focused modules

### Documentation

- Update Ralph loops documentation
- Add user installation methods to CLAUDE.md
- Add crates.io and homebrew installation options

### Fixed

- Ensure v prefix in changelog comparison URLs
- Use hardcoded repo URL in cliff.toml footer
- Remove invalid release_commit_message field
- Escape closes overlays before quitting ([#13](https://github.com/m-mohamed/rehoboam/pull/13))
- Use switch-client for cross-session tmux navigation ([#12](https://github.com/m-mohamed/rehoboam/pull/12))
- Allow dirty CI file for custom release check
- Check if release exists before creating
- Use explicit paths for completions in dist config
- Spawn dialog UX improvements and critical bug fix
[0.9.2]: https://github.com/m-mohamed/rehoboam/compare/v0.9.0...v0.9.2


## [0.9.0] - 2026-01-10

### Added
- **Loop Mode**: Ralph-style autonomous loop control via tmux keystroke injection
  - Spawn agents in loop mode with configurable max iterations and stop word
  - Auto-continues on Stop events by sending Enter via `tmux send-keys`
  - Stall detection: 5+ identical stop reasons triggers STALLED state
  - Circuit breakers: max iterations, stop word detection, stall detection
  - TUI controls: `X` cancels loop, `R` restarts loop
  - Card display shows `Loop N/M`, `STALLED (X/R)`, or `DONE at N`
- **Subagent Visualization**: Parse and render SubagentStart/SubagentStop hooks
  - Subagent struct with id, description, status, duration
  - Cards show subagent count or running subagent description
- **Enhanced Spawn Dialog**: Loop mode toggle, max iterations, stop word fields
- `send_enter()` tmux helper for loop continuation
- `register_loop_config()` for pending spawn configurations
- 5 new stall detection unit tests

### Changed
- Spawn dialog height increased to accommodate loop mode fields
- SpawnState now has 7 fields (added loop_enabled, loop_max_iterations, loop_stop_word)
- `spawn_agent()` now takes `&mut self` for state modification
- Reduced idle timeout from 30s to 10s for faster responsiveness

### Fixed
- Duplicate LoopConfig definition resolved

## [0.8.1] - 2026-01-09

### Added
- 10-second idle timeout: Working → Idle auto-transition when no events received
- 5-minute stale session cleanup: Removes agents that haven't sent events
- WEZTERM_PANE environment passthrough in hook commands
- Debug logging when pane ID falls back to session_id

### Changed
- Hook commands now explicitly pass WEZTERM_PANE for reliable pane jumping

### Fixed
- Agents no longer stuck in Working state indefinitely
- Stale sessions now cleaned up automatically

## [0.8.0] - 2026-01-08

### Added
- Real-time TUI for Claude Code agent monitoring
- Kanban layout with four columns: Attention, Working, Compacting, Idle
- Unix socket IPC for sub-millisecond hook event delivery
- Desktop notifications on permission requests (`-N` flag)
- Multi-terminal support (WezTerm, Kitty, iTerm2)
- Hook installer command (`rehoboam init`)
- Tool latency tracking with Pre→Post correlation
- Activity sparklines per agent
- Status count caching for O(1) header updates
- LRU eviction when agent limit reached (500 max)
- GitHub Actions CI with multi-platform builds

[Unreleased]: https://github.com/m-mohamed/rehoboam/compare/v0.9.0...HEAD
[0.9.0]: https://github.com/m-mohamed/rehoboam/compare/v0.8.1...v0.9.0
[0.8.1]: https://github.com/m-mohamed/rehoboam/compare/v0.8.0...v0.8.1
[0.8.0]: https://github.com/m-mohamed/rehoboam/releases/tag/v0.8.0
