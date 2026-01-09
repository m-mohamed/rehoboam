use crate::event::Event;
use crate::state::AppState;
use crossterm::event::{KeyCode, KeyModifiers};
use std::process::Command;

/// Application state and logic
pub struct App {
    pub state: AppState,
    pub should_quit: bool,
    pub debug_mode: bool,
    pub show_help: bool,
    /// Freeze display - stops UI updates but events still received
    pub frozen: bool,
    /// Dirty flag: true if UI needs re-render (render-on-change optimization)
    pub needs_render: bool,
}

impl App {
    pub fn new(debug_mode: bool) -> Self {
        Self {
            state: AppState::new(),
            should_quit: false,
            debug_mode,
            show_help: false,
            frozen: false,
            needs_render: true, // Always render first frame
        }
    }

    /// Handle incoming events
    pub fn handle_event(&mut self, event: Event) {
        match event {
            Event::Hook(hook_event) => {
                // Only process hook events if not frozen
                if !self.frozen {
                    self.state.process_event(*hook_event);
                    self.needs_render = true; // State changed
                }
            }
            Event::Key(key) => {
                self.handle_key(key);
                self.needs_render = true; // Any key press triggers render
            }
        }
    }

    /// Handle keyboard input
    fn handle_key(&mut self, key: crossterm::event::KeyEvent) {
        // Handle Ctrl+C
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }

        match key.code {
            // Quit
            KeyCode::Char('q') | KeyCode::Esc => {
                self.should_quit = true;
            }
            // Column navigation (horizontal)
            KeyCode::Char('h') | KeyCode::Left => {
                self.state.move_column_left();
            }
            KeyCode::Char('l') | KeyCode::Right => {
                self.state.move_column_right();
            }
            // Card navigation (vertical within column)
            KeyCode::Char('j') | KeyCode::Down => {
                self.state.next_card();
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.state.previous_card();
            }
            // Jump to agent
            KeyCode::Enter => {
                self.jump_to_selected();
            }
            // Toggle help (use '?' since 'h' is now column navigation)
            KeyCode::Char('?') | KeyCode::Char('H') => {
                self.show_help = !self.show_help;
            }
            // Toggle debug mode
            KeyCode::Char('d') => {
                self.debug_mode = !self.debug_mode;
            }
            // Toggle freeze mode (stops UI updates for stable selection)
            KeyCode::Char('f') => {
                self.frozen = !self.frozen;
            }
            _ => {}
        }
    }

    /// Tick for triggering re-renders
    ///
    /// Best practice from ratatui async-template:
    /// - Events update state (hook events push activity data)
    /// - Ticks trigger re-render only (no new data)
    ///
    /// This ensures sparkline consistency - activity values only come
    /// from real hook events, not synthesized tick data.
    pub fn tick(&mut self) {
        // Process timeout-based state transitions
        self.state.tick();
        // Tick triggers re-render for elapsed time updates
        self.needs_render = true;
    }

    /// Called after render to reset dirty flag
    pub fn rendered(&mut self) {
        self.needs_render = false;
    }

    /// Jump to selected agent using wezterm CLI
    fn jump_to_selected(&self) {
        if let Some(agent) = self.state.selected_agent() {
            let pane_id = &agent.pane_id;
            tracing::debug!("Jumping to pane {}", pane_id);

            // Use wezterm CLI to activate pane
            let result = Command::new("wezterm")
                .args(["cli", "activate-pane", "--pane-id", pane_id])
                .output();

            match result {
                Ok(output) => {
                    if !output.status.success() {
                        tracing::warn!(
                            "Failed to activate pane: {}",
                            String::from_utf8_lossy(&output.stderr)
                        );
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to run wezterm CLI: {}", e);
                }
            }
        }
    }
}
