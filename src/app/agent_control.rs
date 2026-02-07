//! Agent control operations: approve, reject, kill, custom input

use crate::sprite::controller::SpriteController;
use crate::state::AppState;
use crate::tmux::TmuxController;
use sprites::SpritesClient;

/// Send a signal to the selected agent
///
/// Handles both tmux and sprite agents with unified logging.
pub fn send_to_selected(
    state: &AppState,
    sprites_client: Option<&SpritesClient>,
    signal: &str,
    action_name: &str,
) {
    let Some(agent) = state.selected_agent() else {
        return;
    };

    let pane_id = &agent.pane_id;

    tracing::info!(
        pane_id = %pane_id,
        project = %agent.project,
        is_sprite = agent.is_sprite,
        action = %action_name,
    );

    if agent.is_sprite {
        send_sprite_signal(sprites_client, pane_id, signal, action_name);
    } else if pane_id.starts_with('%') {
        send_tmux_signal(pane_id, signal, action_name);
    } else {
        tracing::warn!(pane_id = %pane_id, "Cannot {}: unknown pane type", action_name);
    }
}

/// Send a signal to a sprite agent
fn send_sprite_signal(
    sprites_client: Option<&SpritesClient>,
    pane_id: &str,
    signal: &str,
    action_name: &str,
) {
    let Some(client) = sprites_client else {
        tracing::warn!(
            pane_id = %pane_id,
            "Cannot {} sprite: sprites client not configured",
            action_name
        );
        return;
    };

    let sprite = client.sprite(pane_id);
    let action_name = action_name.to_string();

    match signal {
        "y" => {
            tokio::spawn(async move {
                if let Err(e) = SpriteController::approve(&sprite).await {
                    tracing::error!(error = %e, "Sprite {} failed", action_name);
                }
            });
        }
        "n" => {
            tokio::spawn(async move {
                if let Err(e) = SpriteController::reject(&sprite).await {
                    tracing::error!(error = %e, "Sprite {} failed", action_name);
                }
            });
        }
        "C-c" => {
            tokio::spawn(async move {
                if let Err(e) = SpriteController::kill(&sprite).await {
                    tracing::error!(error = %e, "Sprite {} failed", action_name);
                }
            });
        }
        _ => {
            tracing::warn!(signal = %signal, "Unknown signal for sprite");
        }
    }
}

/// Send a signal to a tmux pane
fn send_tmux_signal(pane_id: &str, signal: &str, action_name: &str) {
    let result = if signal == "C-c" {
        TmuxController::send_keys_raw(pane_id, signal)
    } else {
        TmuxController::send_keys(pane_id, signal)
    };

    if let Err(e) = result {
        tracing::error!(
            pane_id = %pane_id,
            error = %e,
            "Failed to send {}",
            action_name
        );
    }
}

/// Approve permission request for selected agent
pub fn approve_selected(state: &AppState, sprites_client: Option<&SpritesClient>) {
    send_to_selected(state, sprites_client, "y", "approval");
}

/// Reject permission request for selected agent
pub fn reject_selected(state: &AppState, sprites_client: Option<&SpritesClient>) {
    send_to_selected(state, sprites_client, "n", "rejection");
}

/// Bulk send a signal to all selected agents
///
/// Handles both tmux and sprite agents with unified logging.
/// Returns a status message if some agents were skipped.
pub fn bulk_send_signal(
    state: &mut AppState,
    sprites_client: Option<&SpritesClient>,
    tmux_signal: &str,
    action_name: &str,
) -> Option<String> {
    let tmux_panes = state.selected_tmux_panes();
    let sprite_agents = state.selected_sprite_agents();

    if tmux_panes.is_empty() && sprite_agents.is_empty() {
        tracing::warn!("No agents selected for bulk {}", action_name);
        state.clear_selection();
        return Some(format!("No agents selected for {}", action_name));
    }

    let mut sent = 0usize;
    let mut skipped = 0usize;

    // Handle tmux agents
    if !tmux_panes.is_empty() {
        tracing::info!(count = tmux_panes.len(), "Bulk {} tmux agents", action_name);
        for pane_id in &tmux_panes {
            let result = if tmux_signal == "C-c" {
                TmuxController::send_keys_raw(pane_id, tmux_signal)
            } else {
                TmuxController::send_keys(pane_id, tmux_signal)
            };
            if let Err(e) = result {
                tracing::error!(pane_id = %pane_id, error = %e, "Failed to {}", action_name);
            } else {
                sent += 1;
            }
        }
    }

    // Handle sprite agents
    if !sprite_agents.is_empty() {
        if let Some(client) = sprites_client {
            tracing::info!(
                count = sprite_agents.len(),
                "Bulk {} sprite agents",
                action_name
            );
            for sprite_id in &sprite_agents {
                send_sprite_signal(Some(client), sprite_id, tmux_signal, action_name);
                sent += 1;
            }
        } else {
            skipped = sprite_agents.len();
            tracing::warn!(
                count = skipped,
                "Cannot {} sprites: sprites client not configured",
                action_name
            );
        }
    }

    state.clear_selection();

    if skipped > 0 {
        Some(format!(
            "Bulk {}: {} sent, {} sprite(s) skipped (no token)",
            action_name, sent, skipped
        ))
    } else {
        None
    }
}

/// Bulk approve all selected agents
pub fn bulk_approve(state: &mut AppState, sprites_client: Option<&SpritesClient>) -> Option<String> {
    bulk_send_signal(state, sprites_client, "y", "approval")
}

/// Bulk reject all selected agents
pub fn bulk_reject(state: &mut AppState, sprites_client: Option<&SpritesClient>) -> Option<String> {
    bulk_send_signal(state, sprites_client, "n", "rejection")
}

/// Bulk kill all selected agents (send Ctrl+C)
pub fn bulk_kill(state: &mut AppState, sprites_client: Option<&SpritesClient>) -> Option<String> {
    bulk_send_signal(state, sprites_client, "C-c", "kill")
}

/// Send custom input to selected agent
///
/// Uses buffered send for multi-line or long input.
/// For sprite agents, uses async `SpriteController::send_input`.
pub fn send_custom_input(state: &AppState, sprites_client: Option<&SpritesClient>, input: &str) {
    if input.is_empty() {
        return;
    }

    let Some(agent) = state.selected_agent() else {
        return;
    };

    let pane_id = &agent.pane_id;

    tracing::info!(
        pane_id = %pane_id,
        project = %agent.project,
        input_len = input.len(),
        is_sprite = agent.is_sprite,
        "Sending custom input"
    );

    if agent.is_sprite {
        // Sprite agents: send via SpriteController async
        let Some(client) = sprites_client else {
            tracing::warn!(
                pane_id = %pane_id,
                "Cannot send sprite input: sprites client not configured"
            );
            return;
        };

        let sprite = client.sprite(pane_id);
        let input = input.to_string();

        tokio::spawn(async move {
            if let Err(e) = SpriteController::send_input(&sprite, &input).await {
                tracing::error!(error = %e, "Failed to send sprite custom input");
            }
        });
    } else if pane_id.starts_with('%') {
        // Tmux panes: send directly
        // Use buffered send for multi-line or long input, simple send for short
        let result = if input.contains('\n') || input.len() > 100 {
            TmuxController::send_buffered(pane_id, input)
        } else {
            TmuxController::send_keys(pane_id, input)
        };

        if let Err(e) = result {
            tracing::error!(
                pane_id = %pane_id,
                error = %e,
                "Failed to send custom input"
            );
        }
    } else {
        tracing::warn!(
            pane_id = %pane_id,
            "Cannot send input: unknown pane type"
        );
    }
}
