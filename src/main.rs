//! Rehoboam - Real-time observability TUI for Claude Code agents
//!
//! Named after Westworld's AI that tracks and predicts every human's path.
//! A single Rust binary that provides both:
//! - TUI mode: Real-time dashboard for monitoring Claude Code agents
//! - Hook mode: Called by hooks to parse stdin JSON and send events to the TUI
//!
//! Usage:
//!   rehoboam          # Start TUI (default)
//!   rehoboam hook     # Process hook event from stdin (Claude Code pipes JSON)

mod action;
mod app;
mod cli;
mod config;
mod errors;
mod event;
mod git;
mod init;
mod notify;
mod sprite;
mod state;
mod tmux;
mod tui;
mod ui;

use app::App;
use clap::Parser;
use cli::{Cli, Commands};
use color_eyre::Result;
use std::path::PathBuf;
use std::process::Command;
use tokio::sync::mpsc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Length of session_id prefix used as fallback pane identifier
const SESSION_ID_PREFIX_LEN: usize = 8;

/// Get the log directory path
fn get_log_dir() -> PathBuf {
    directories::BaseDirs::new()
        .map(|dirs| dirs.cache_dir().join("rehoboam").join("logs"))
        .unwrap_or_else(|| PathBuf::from("/tmp/rehoboam/logs"))
}

/// Install the binary to ~/.local/bin/
fn install_binary() -> Result<()> {
    let current_exe = std::env::current_exe()?;
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let install_dir = PathBuf::from(home).join(".local").join("bin");
    let install_path = install_dir.join("rehoboam");

    // Create directory if needed
    std::fs::create_dir_all(&install_dir)?;

    // Copy binary
    std::fs::copy(&current_exe, &install_path)?;

    // Make executable (Unix only)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&install_path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&install_path, perms)?;
    }

    println!("Installed to: {}", install_path.display());
    println!("Make sure ~/.local/bin is in your PATH");
    Ok(())
}

/// Handle Claude Code hook event (v1.0)
///
/// Reads JSON from stdin (piped by Claude Code hooks), parses all available fields,
/// enriches with terminal context, and sends to TUI via Unix socket.
///
/// Agent identification priority:
/// 1. WEZTERM_PANE (WezTerm)
/// 2. KITTY_WINDOW_ID (Kitty)
/// 3. ITERM_SESSION_ID (iTerm2)
/// 4. session_id prefix (any terminal)
///
/// Silently succeeds if:
/// - No stdin input (empty hook call)
/// - Socket unavailable (TUI not running)
async fn handle_hook(socket_path: &PathBuf, should_notify: bool) -> Result<()> {
    use std::io::{self, BufRead};
    use tokio::io::AsyncWriteExt;
    use tokio::net::UnixStream as TokioUnixStream;
    use tokio::time::{timeout, Duration};

    // Read JSON from stdin (Claude Code pipes it)
    let stdin = io::stdin();
    let mut input = String::new();
    for line in stdin.lock().lines().map_while(Result::ok) {
        input.push_str(&line);
    }

    if input.trim().is_empty() {
        return Ok(()); // Silent exit - no input
    }

    // Parse Claude Code's hook JSON
    let hook_input: event::ClaudeHookInput = match serde_json::from_str(&input) {
        Ok(parsed) => parsed,
        Err(e) => {
            tracing::debug!("Failed to parse hook JSON: {}", e);
            return Ok(()); // Silent exit - invalid JSON
        }
    };

    // Get pane ID from terminal-specific env vars, fall back to session_id
    // Priority: WEZTERM_PANE > TMUX_PANE > KITTY_WINDOW_ID > ITERM_SESSION_ID > session_id
    let wezterm_pane = std::env::var("WEZTERM_PANE").ok();
    let tmux_pane = std::env::var("TMUX_PANE").ok();
    let pane_id = wezterm_pane
        .clone()
        .or_else(|| tmux_pane.clone()) // Tmux: %0, %1, etc.
        .or_else(|| std::env::var("KITTY_WINDOW_ID").ok())
        .or_else(|| std::env::var("ITERM_SESSION_ID").ok())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            // Fallback: first 8 chars of session_id (always available)
            tracing::debug!(
                "No terminal pane ID found (WEZTERM_PANE={:?}, TMUX_PANE={:?}), using session_id fallback",
                wezterm_pane,
                tmux_pane
            );
            hook_input.session_id.chars().take(SESSION_ID_PREFIX_LEN).collect()
        });

    // Derive status from hook event name
    let (status, attention_type) = hook_input.derive_status();

    // Get project name
    let project = get_project_name();

    // Get current timestamp (milliseconds for precision)
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    // Build enriched socket message with ALL v1.0 fields
    let socket_event = event::HookEvent {
        event: hook_input.hook_event_name.clone(),
        status: status.to_string(),
        attention_type: attention_type.map(String::from),
        pane_id: pane_id.clone(),
        project: project.clone(),
        timestamp,
        // v1.0 rich data fields
        session_id: Some(hook_input.session_id.clone()),
        tool_name: hook_input.tool_name.clone(),
        tool_input: hook_input.tool_input.clone(),
        tool_use_id: hook_input.tool_use_id.clone(),
        // v0.9.0 loop mode fields
        reason: hook_input.reason.clone(),
        // v0.9.0 subagent fields
        subagent_id: hook_input.subagent_id.clone(),
        description: hook_input.description.clone(),
        subagent_duration_ms: hook_input.duration_ms,
        // v0.10.0 sprite fields
        source: event::EventSource::Local,
    };

    // Try to send to TUI via socket (non-blocking, best effort)
    if socket_path.exists() {
        let connect_result = timeout(
            Duration::from_millis(500),
            TokioUnixStream::connect(socket_path),
        )
        .await;

        if let Ok(Ok(mut stream)) = connect_result {
            if let Ok(json) = serde_json::to_string(&socket_event) {
                let data = format!("{}\n", json);
                let _ = timeout(
                    Duration::from_millis(500),
                    stream.write_all(data.as_bytes()),
                )
                .await;
                let _ = stream.shutdown().await;
            }
        }
    }

    // Send desktop notification if requested
    if should_notify {
        match status {
            "attention" => {
                let msg = match attention_type {
                    Some("notification") => hook_input
                        .message
                        .unwrap_or_else(|| "Notification".to_string()),
                    _ => format!("Approve in {}", project),
                };
                notify::send("Claude Needs Attention", &msg, Some("Basso"));
            }
            "idle" if hook_input.hook_event_name == "Stop" => {
                let reason = hook_input.reason.unwrap_or_else(|| "Complete".to_string());
                notify::send(
                    "Claude Done",
                    &format!("{}: {}", project, reason),
                    Some("default"),
                );
            }
            _ => {}
        }
    }

    Ok(())
}

/// Get project name from CLAUDE_PROJECT_DIR, git repo, or current directory
fn get_project_name() -> String {
    // Try CLAUDE_PROJECT_DIR first (always set by Claude Code 2.1.0+)
    if let Ok(dir) = std::env::var("CLAUDE_PROJECT_DIR") {
        if let Some(name) = std::path::Path::new(&dir).file_name() {
            return name.to_string_lossy().to_string();
        }
    }

    // Fall back to git
    if let Ok(output) = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
    {
        if output.status.success() {
            if let Ok(path) = String::from_utf8(output.stdout) {
                if let Some(name) = path.trim().rsplit('/').next() {
                    return name.to_string();
                }
            }
        }
    }

    // Fall back to current directory name
    std::env::current_dir()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| "unknown".to_string())
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse CLI arguments
    let cli = Cli::parse();

    // Handle --install flag
    if cli.install {
        return install_binary();
    }

    // Handle subcommands
    match cli.command {
        Some(Commands::Hook { notify }) => {
            // Hook mode: read stdin JSON, enrich with context, send to TUI
            return handle_hook(&cli.socket, notify).await;
        }
        Some(Commands::Init {
            path,
            all,
            list,
            force,
        }) => {
            // Init mode: install hooks to project(s)
            return init::run(path, all, list, force).map_err(|e| color_eyre::eyre::eyre!("{}", e));
        }
        Some(Commands::Completions { shell }) => {
            // Generate shell completions
            cli::print_completions(shell);
            return Ok(());
        }
        None => {
            // TUI mode: continue with full setup
        }
    }

    // Initialize error handling
    color_eyre::install()?;

    // Setup file logging with rotation
    let log_dir = get_log_dir();
    std::fs::create_dir_all(&log_dir)?;

    let file_appender = tracing_appender::rolling::daily(&log_dir, "rehoboam.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    // Initialize logging with both file and stderr
    let log_filter = format!("rehoboam={}", cli.log_level);
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(&log_filter))
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(true)
                .with_writer(non_blocking),
        )
        .init();

    tracing::info!("Starting rehoboam v{}", env!("CARGO_PKG_VERSION"));
    tracing::info!("Log directory: {:?}", log_dir);
    tracing::debug!("Socket path: {:?}", cli.socket);

    // Create event channel
    let (event_tx, event_rx) = mpsc::channel(100);

    // Spawn socket listener
    let socket_path = cli.socket.clone();
    let socket_tx = event_tx.clone();
    let socket_handle = tokio::spawn(async move {
        if let Err(e) = event::socket::listen(socket_tx, &socket_path).await {
            tracing::error!("Socket listener error: {}", e);
        }
    });

    // Optionally spawn sprite event forwarder (WebSocket server for remote sprites)
    let sprite_handle = if cli.enable_sprites {
        // Validate sprites token is provided
        if cli.sprites_token.is_none() {
            return Err(color_eyre::eyre::eyre!(
                "--sprites-token or SPRITES_TOKEN env required when --enable-sprites is set"
            ));
        }

        tracing::info!(
            "Sprite support enabled, WebSocket server on port {}",
            cli.sprite_ws_port
        );

        // Spawn the WebSocket forwarder
        let (mut sprite_rx, forwarder_handle) =
            sprite::forwarder::spawn_forwarder(cli.sprite_ws_port);

        // Spawn task to convert RemoteHookEvent -> Event::RemoteHook
        let sprite_event_tx = event_tx.clone();
        let converter_handle = tokio::spawn(async move {
            while let Some(remote_event) = sprite_rx.recv().await {
                // Convert forwarder's RemoteHookEvent to our HookEvent
                let hook_event = event::HookEvent {
                    event: remote_event
                        .event
                        .hook_event_name
                        .unwrap_or_else(|| "Unknown".to_string()),
                    status: "working".to_string(), // Will be derived by app
                    attention_type: None,
                    pane_id: remote_event.sprite_id.clone(),
                    project: "sprite".to_string(), // TODO: extract from sprite metadata
                    timestamp: remote_event.timestamp.unwrap_or_else(|| {
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs() as i64)
                            .unwrap_or(0)
                    }),
                    session_id: remote_event.event.session_id,
                    tool_name: remote_event.event.tool_name,
                    tool_input: remote_event.event.tool_input,
                    tool_use_id: None,
                    reason: None,
                    subagent_id: None,
                    description: None,
                    subagent_duration_ms: None,
                    source: event::EventSource::Sprite {
                        sprite_id: remote_event.sprite_id.clone(),
                    },
                };

                // Send as RemoteHook event
                if let Err(e) = sprite_event_tx
                    .send(event::Event::RemoteHook {
                        sprite_id: remote_event.sprite_id,
                        event: Box::new(hook_event),
                    })
                    .await
                {
                    tracing::error!("Failed to forward sprite event: {}", e);
                    break;
                }
            }
        });

        Some((forwarder_handle, converter_handle))
    } else {
        None
    };

    // Run TUI
    let result = run_tui(event_tx, event_rx, cli.debug, cli.tick_rate, cli.frame_rate).await;

    // Cleanup
    socket_handle.abort();

    // Cleanup sprite handles if enabled
    if let Some((forwarder_handle, converter_handle)) = sprite_handle {
        forwarder_handle.abort();
        converter_handle.abort();
        tracing::debug!("Sprite forwarder shut down");
    }

    // Remove socket file
    if cli.socket.exists() {
        let _ = std::fs::remove_file(&cli.socket);
    }

    result
}

async fn run_tui(
    event_tx: mpsc::Sender<event::Event>,
    mut event_rx: mpsc::Receiver<event::Event>,
    debug_mode: bool,
    tick_rate: f64,
    frame_rate: f64,
) -> Result<()> {
    use std::time::{Duration, Instant};
    use tokio_util::sync::CancellationToken;

    // Calculate durations from rates
    let tick_duration = Duration::from_secs_f64(1.0 / tick_rate);
    let frame_duration = Duration::from_secs_f64(1.0 / frame_rate);

    tracing::info!(
        "TUI starting: {:.1} FPS, {:.1} ticks/sec",
        frame_rate,
        tick_rate
    );

    // Initialize terminal (raw mode, alternate screen, mouse capture)
    let mut terminal = tui::init()?;

    // RAII guard ensures terminal is restored on panic or early return
    let _guard = tui::TerminalGuard;

    // Create app state
    let mut app = App::new(debug_mode);

    // Create cancellation token for graceful shutdown
    let cancel = CancellationToken::new();

    // Spawn input event handler with cancellation support
    let input_tx = event_tx.clone();
    let input_cancel = cancel.clone();
    let input_handle = tokio::spawn(async move {
        event::input::listen(input_tx, input_cancel).await;
    });

    // Frame rate limiting state
    let mut last_frame = Instant::now();

    // Main loop
    loop {
        // Frame rate limiting with dirty flag check
        let now = Instant::now();
        if app.needs_render && now.duration_since(last_frame) >= frame_duration {
            terminal.draw(|f| ui::render(f, &app))?;
            app.rendered();
            last_frame = now;
        }

        // Handle events with tick-based timeout
        tokio::select! {
            Some(event) = event_rx.recv() => {
                app.handle_event(event);
            }
            _ = tokio::time::sleep(tick_duration) => {
                app.tick();
            }
        }

        if app.should_quit {
            break;
        }
    }

    // Graceful shutdown: signal input listener to stop
    tracing::debug!("Shutting down input listener");
    cancel.cancel();
    input_handle.abort();

    // Restore terminal (guard will also restore on drop, but explicit is cleaner)
    tui::restore()?;
    terminal.show_cursor()?;

    Ok(())
}
