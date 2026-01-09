# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.8.0] - 2026-01-08

### Added
- **Configurable frame rate**: `--frame-rate` / `-F` flag (default: 30 FPS)
- **Configurable tick rate**: `--tick-rate` / `-t` flag (default: 1.0 ticks/sec)
- **Render-on-change**: Dirty flag pattern skips redundant renders
- **Socket buffer tuning**: 4KB recv buffer via socket2 (vs 128KB+ default)

### Changed
- Frame rate limiting with `Instant`-based timing
- Event enum uses `Box<HookEvent>` to reduce enum size (232 → 16 bytes)

### Fixed
- CI release job artifact rename script

### Performance
- Reduced CPU usage when idle (no events = no render)
- Smoother animations with consistent frame timing
- Lower memory per Event variant

## [0.7.0] - 2026-01-08

### Added
- **Multi-terminal support**: Works with WezTerm, Kitty, iTerm2, or any terminal
- Terminal env var fallback chain: `WEZTERM_PANE` → `KITTY_WINDOW_ID` → `ITERM_SESSION_ID` → `session_id`

### Changed
- No longer exits silently when terminal env var is missing
- Uses first 8 characters of Claude Code's `session_id` as universal fallback

### Fixed
- Agents now tracked in any terminal, not just WezTerm

## [0.6.0] - 2026-01-08

### Added
- **MAX_AGENTS limit (500)** with LRU eviction (prefers idle agents)
- Cached `status_counts[4]` in AppState for O(1) header lookups

### Changed
- **render_header()**: Uses cached counts instead of 4x iteration (O(1) vs O(4n))
- **render_activity()**: Flattens already-sorted columns instead of O(n log n) sort
- Backwards-compatible methods (sorted_agents, next, previous) marked `#[allow(dead_code)]`

### Performance
- ~40% reduction in render time at scale (50+ agents)
- Header status counts: O(4n) → O(1)
- Activity rendering: O(n log n) → O(n) (flatten vs sort)

## [0.5.0] - 2026-01-08

### Added
- **Kanban-style column layout** with 4 status columns:
  - Attention - needs user input
  - Working - actively processing
  - Compacting - context compaction
  - Idle - waiting
- Horizontal column navigation with `h/l` keys
- Vertical card navigation within columns with `j/k` keys
- Per-column card selection

### Changed
- UI redesigned from flat list to Kanban board
- Navigation now 2D (columns + cards) instead of 1D list

## [0.4.0] - 2026-01-07

### Added
- Native desktop notifications via `-N` flag (macOS + Linux)
- `init` subcommand for hook installation
  - Git-based project discovery
  - Safe merge with existing settings
  - Interactive multi-project selection (`--all`)
- Full hook data parsing (session_id, tool_use_id, tool_name)
- Tool latency tracking (PreToolUse → PostToolUse correlation)
- Session lifecycle tracking

### Changed
- Hook commands now use `-N` flag instead of inline terminal-notifier

## [0.3.0] - 2026-01-08

### Changed
- Comprehensive audit improvements for production quality
- Code cleanup and refactoring

## [0.2.0] - 2026-01-08

### Added
- SubagentStart hook support
- Configurable project discovery
- Native notifications integration

## [0.1.0] - 2026-01-06

### Added
- First release
- Core architecture: hooks -> socket -> daemon -> TUI
- Support for all Claude Code hook events
- Keyboard navigation (j/k, q, Enter)
- Jump to agent via wezterm cli
- GitHub Actions CI workflow

[Unreleased]: https://github.com/m-mohamed/rehoboam/compare/v0.8.0...HEAD
[0.8.0]: https://github.com/m-mohamed/rehoboam/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/m-mohamed/rehoboam/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/m-mohamed/rehoboam/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/m-mohamed/rehoboam/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/m-mohamed/rehoboam/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/m-mohamed/rehoboam/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/m-mohamed/rehoboam/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/m-mohamed/rehoboam/releases/tag/v0.1.0
