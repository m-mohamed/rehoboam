//! Terminal setup and management
//!
//! Handles terminal initialization, restoration, and provides RAII guards
//! for safe cleanup on exit or panic.

use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;
use std::io::{self, stdout, Stdout};

/// Type alias for our terminal backend
pub type Tui = Terminal<CrosstermBackend<Stdout>>;

/// Initialize terminal for TUI mode
///
/// Sets up raw mode, alternate screen, and mouse capture.
/// Returns a configured terminal ready for rendering.
///
/// # Errors
/// Returns error if terminal setup fails (e.g., not a TTY).
pub fn init() -> io::Result<Tui> {
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    Terminal::new(CrosstermBackend::new(stdout))
}

/// Restore terminal to normal state
///
/// Disables raw mode, exits alternate screen, and disables mouse capture.
/// Safe to call multiple times.
pub fn restore() -> io::Result<()> {
    disable_raw_mode()?;
    execute!(stdout(), LeaveAlternateScreen, DisableMouseCapture)?;
    Ok(())
}

/// RAII guard that restores terminal state on drop
///
/// This ensures the terminal is properly restored even if the TUI panics,
/// preventing the user from being left with a broken terminal state.
///
/// # Usage
/// ```ignore
/// let _guard = TerminalGuard;
/// // ... TUI code that might panic ...
/// // Terminal is automatically restored when guard goes out of scope
/// ```
pub struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = restore();
    }
}
