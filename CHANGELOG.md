# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- 30-second idle timeout: Working → Idle auto-transition when no events received
- 5-minute stale session cleanup: Removes agents that haven't sent events
- WEZTERM_PANE environment passthrough in hook commands
- Debug logging when pane ID falls back to session_id
- "Why Rehoboam?" section in README
- Troubleshooting section in README
- Configuration section in README
- Hook Event Schema documentation in ARCHITECTURE.md

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

[Unreleased]: https://github.com/m-mohamed/rehoboam/compare/v0.8.0...HEAD
[0.8.0]: https://github.com/m-mohamed/rehoboam/releases/tag/v0.8.0
