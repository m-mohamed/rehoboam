// Clippy configuration: enable pedantic but allow overly strict lints
#![allow(clippy::missing_errors_doc)] // Internal functions don't need # Errors docs
#![allow(clippy::missing_panics_doc)] // Internal functions don't need # Panics docs
#![allow(clippy::must_use_candidate)] // Not all getters need #[must_use]
#![allow(clippy::module_name_repetitions)] // e.g., SpriteConfig in sprite module is fine
#![allow(clippy::doc_markdown)] // Don't require backticks around WezTerm, JSON, etc.
#![allow(clippy::too_many_lines)] // Some functions are naturally long
#![allow(clippy::struct_excessive_bools)] // Config structs can have multiple bool fields
#![allow(clippy::similar_names)] // Allow similar variable names like tmux/tmux_pane
#![allow(clippy::cast_possible_truncation)] // We're careful with our casts
#![allow(clippy::cast_sign_loss)] // Timestamp conversions are safe
#![allow(clippy::cast_precision_loss)] // Duration to f64 precision loss is acceptable
#![allow(clippy::significant_drop_tightening)] // Lock guard drops are intentional
#![allow(clippy::redundant_closure_for_method_calls)] // Sometimes closures are clearer
#![allow(clippy::if_not_else)] // Negative conditions can be clearer for early returns
#![allow(clippy::match_same_arms)] // Explicit arms are clearer than combined patterns
#![allow(clippy::single_match_else)] // match with else is fine for Result handling
#![allow(clippy::manual_let_else)] // if-let is clearer for multi-line error handling
#![allow(clippy::items_after_statements)] // Helper closures can be defined inline
#![allow(clippy::option_if_let_else)] // if-let is more readable for Option handling
#![allow(clippy::unnecessary_wraps)] // Some functions return Result for consistency
#![allow(clippy::needless_pass_by_value)] // PathBuf by value is fine for config loading
#![allow(clippy::trivially_copy_pass_by_ref)] // &self on Copy types follows Rust conventions
#![allow(clippy::cast_possible_wrap)] // Timestamp u64->i64 won't overflow until year 292 billion
#![allow(clippy::assigning_clones)] // .clone() is clearer than .clone_from() in most cases

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

mod app;
mod cli;
mod config;
mod diff;
mod errors;
mod event;
mod git;
mod init;
mod notify;
mod reconcile;
mod rehoboam_loop;
mod sprite;
mod state;
#[allow(dead_code)]
mod telemetry;
mod tmux;
mod tui;
mod ui;

use app::App;
use clap::Parser;
use cli::{Cli, Commands, SpritesAction};
use color_eyre::Result;
use std::path::PathBuf;
use std::process::Command;
use tokio::sync::mpsc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Length of session_id prefix used as fallback pane identifier
const SESSION_ID_PREFIX_LEN: usize = 8;

/// Get the log directory path
fn get_log_dir() -> PathBuf {
    directories::BaseDirs::new().map_or_else(
        || PathBuf::from("/tmp/rehoboam/logs"),
        |dirs| dirs.cache_dir().join("rehoboam").join("logs"),
    )
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

/// Handle sprites management commands
async fn handle_sprites_command(action: SpritesAction, token: Option<String>) -> Result<()> {
    let token = token.ok_or_else(|| {
        color_eyre::eyre::eyre!("SPRITES_TOKEN required. Set env var or use --sprites-token")
    })?;

    let client = sprites::SpritesClient::new(&token);

    match action {
        SpritesAction::List => {
            let sprites = client
                .list()
                .await
                .map_err(|e| color_eyre::eyre::eyre!("Failed to list sprites: {}", e))?;

            if sprites.is_empty() {
                println!("No sprites found");
            } else {
                println!("{:<30} {:<12} {:<20}", "NAME", "STATUS", "CREATED");
                println!("{}", "-".repeat(62));
                for sprite in sprites {
                    let created = sprite
                        .created_at
                        .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
                        .unwrap_or_else(|| "-".to_string());
                    println!(
                        "{:<30} {:<12} {:<20}",
                        sprite.name,
                        format!("{:?}", sprite.status),
                        created
                    );
                }
            }
        }
        SpritesAction::Info { name } => {
            let info = client
                .get(&name)
                .await
                .map_err(|e| color_eyre::eyre::eyre!("Failed to get sprite '{}': {}", name, e))?;
            println!("Name:    {}", info.name);
            println!("Status:  {:?}", info.status);
            if let Some(created) = info.created_at {
                println!("Created: {}", created.format("%Y-%m-%d %H:%M:%S"));
            }
        }
        SpritesAction::Destroy { name } => {
            client.delete(&name).await.map_err(|e| {
                color_eyre::eyre::eyre!("Failed to destroy sprite '{}': {}", name, e)
            })?;
            println!("Destroyed: {}", name);
        }
        SpritesAction::DestroyAll { yes } => {
            let sprites = client
                .list()
                .await
                .map_err(|e| color_eyre::eyre::eyre!("Failed to list sprites: {}", e))?;

            if sprites.is_empty() {
                println!("No sprites to destroy");
                return Ok(());
            }

            if !yes {
                println!("This will destroy {} sprites:", sprites.len());
                for sprite in &sprites {
                    println!("  - {} ({:?})", sprite.name, sprite.status);
                }
                print!("Continue? [y/N] ");
                use std::io::Write;
                std::io::stdout().flush()?;

                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                if !input.trim().eq_ignore_ascii_case("y") {
                    println!("Cancelled");
                    return Ok(());
                }
            }

            for sprite in sprites {
                match client.delete(&sprite.name).await {
                    Ok(()) => println!("Destroyed: {}", sprite.name),
                    Err(e) => eprintln!("Failed to destroy {}: {}", sprite.name, e),
                }
            }
        }
    }

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
async fn handle_hook(
    socket_path: &PathBuf,
    should_notify: bool,
    inject_context: bool,
) -> Result<()> {
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
            tracing::warn!(
                error = %e,
                input_len = input.len(),
                "Invalid hook JSON from Claude Code (check hook configuration)"
            );
            return Ok(()); // Exit - invalid JSON
        }
    };

    // Handle PermissionRequest events in loop mode - auto-approve based on policy
    // This must be checked before other processing to return early with decision
    if hook_input.hook_event_name == "PermissionRequest" {
        if let Some(loop_dir) = rehoboam_loop::find_rehoboam_dir() {
            // Get project directory for step-up checks
            let project_dir = loop_dir.parent().map(std::path::Path::to_path_buf);

            // Evaluate permission against policy
            let decision = rehoboam_loop::evaluate_permission(
                &loop_dir,
                hook_input.tool_name.as_deref().unwrap_or("Unknown"),
                hook_input.tool_input.as_ref(),
                project_dir.as_deref(),
            );

            // If we have a decision, output it and return early
            if let Some(decision_value) = decision.as_json_value() {
                let output = serde_json::json!({
                    "permissionDecision": decision_value
                });
                println!("{}", serde_json::to_string(&output)?);

                tracing::info!(
                    tool = ?hook_input.tool_name,
                    decision = decision_value,
                    "Permission auto-decided in loop mode"
                );

                // Don't return early - still want to send event to TUI
                // But the permission decision has been output
            }
        }
    }

    // Claude Code 2.1.x: Inject additionalContext for loop mode
    // Only applies to PreToolUse and PostToolUse hooks
    if inject_context {
        let is_tool_hook = matches!(
            hook_input.hook_event_name.as_str(),
            "PreToolUse" | "PostToolUse"
        );

        if is_tool_hook {
            if let Some(loop_dir) = rehoboam_loop::find_rehoboam_dir() {
                if let Ok(context) = rehoboam_loop::build_loop_context(&loop_dir) {
                    // Output JSON that Claude Code will inject
                    let output = serde_json::json!({
                        "additionalContext": context
                    });
                    println!("{}", serde_json::to_string(&output)?);
                    // Continue processing - we still want to update the TUI
                }
            }
        }
    }

    // Get pane ID from terminal-specific env vars, fall back to session_id
    // Priority: WEZTERM_PANE > TMUX_PANE > KITTY_WINDOW_ID > ITERM_SESSION_ID > session_id
    let wezterm_pane = std::env::var("WEZTERM_PANE").ok();
    let tmux_pane = std::env::var("TMUX_PANE").ok();

    // Clone for debug logging if fallback is needed
    let wezterm_debug = wezterm_pane.clone();
    let tmux_debug = tmux_pane.clone();

    let pane_id = wezterm_pane
        .or(tmux_pane) // Tmux: %0, %1, etc.
        .or_else(|| std::env::var("KITTY_WINDOW_ID").ok())
        .or_else(|| std::env::var("ITERM_SESSION_ID").ok())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            // Fallback: first 8 chars of session_id (always available)
            tracing::debug!(
                "No terminal pane ID found (WEZTERM_PANE={:?}, TMUX_PANE={:?}), using session_id fallback",
                wezterm_debug,
                tmux_debug
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
        // Claude Code 2.1.x fields
        context_window: hook_input.context_window.clone(),
        agent_type: hook_input.agent_type.clone(),
        permission_mode: hook_input.permission_mode.clone(),
        cwd: hook_input.cwd.clone(),
        transcript_path: hook_input.transcript_path.clone(),
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
                let data = format!("{json}\n");
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
        match (status, attention_type) {
            // Permission request - needs user approval
            ("attention", Some("permission")) => {
                notify::send(
                    "Claude Needs Attention",
                    &format!("Approve in {project}"),
                    Some("Basso"),
                );
            }
            // Input request - waiting for user response
            ("attention", Some("input")) => {
                notify::send(
                    "Claude Needs Attention",
                    &format!("Input needed in {project}"),
                    Some("Basso"),
                );
            }
            // Notification from Claude
            ("attention", Some("notification")) => {
                let msg = hook_input
                    .message
                    .unwrap_or_else(|| "Notification".to_string());
                notify::send("Claude Notification", &msg, Some("default"));
            }
            // Waiting (was idle) - only notify on Stop event (completion)
            ("attention", Some("waiting")) if hook_input.hook_event_name == "Stop" => {
                let reason = hook_input.reason.unwrap_or_else(|| "Complete".to_string());
                notify::send(
                    "Claude Done",
                    &format!("{project}: {reason}"),
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
        Some(Commands::Hook {
            notify,
            inject_context,
        }) => {
            // Hook mode: read stdin JSON, enrich with context, send to TUI
            return handle_hook(&cli.socket, notify, inject_context).await;
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
        Some(Commands::Sprites { action }) => {
            // Sprites management commands
            return handle_sprites_command(action, cli.sprites_token).await;
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

        // Spawn the WebSocket forwarder with status channel
        let (mut sprite_rx, mut status_rx, forwarder_handle) =
            sprite::forwarder::spawn_forwarder_with_status(cli.sprite_ws_port);

        // Spawn task to forward ConnectionStatus -> Event::SpriteStatus
        let status_event_tx = event_tx.clone();
        let status_handle = tokio::spawn(async move {
            use sprite::forwarder::ConnectionStatus;
            while let Some(status) = status_rx.recv().await {
                let (sprite_id, status_type) = match status {
                    ConnectionStatus::Connected { sprite_id, addr } => {
                        tracing::info!(sprite_id = %sprite_id, addr = %addr, "Sprite connected");
                        (sprite_id, event::SpriteStatusType::Connected)
                    }
                    ConnectionStatus::Disconnected { sprite_id, reason } => {
                        tracing::info!(sprite_id = %sprite_id, reason = %reason, "Sprite disconnected");
                        (sprite_id, event::SpriteStatusType::Disconnected)
                    }
                };

                if let Err(e) = status_event_tx
                    .send(event::Event::SpriteStatus {
                        sprite_id,
                        status: status_type,
                    })
                    .await
                {
                    tracing::error!("Failed to forward sprite status: {}", e);
                    break;
                }
            }
        });

        // Spawn task to convert RemoteHookEvent -> Event::RemoteHook
        let sprite_event_tx = event_tx.clone();
        let converter_handle = tokio::spawn(async move {
            while let Some(remote_event) = sprite_rx.recv().await {
                // Convert forwarder's RemoteHookEvent to our HookEvent
                let hook_name = remote_event
                    .event
                    .hook_event_name
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_string());

                // Derive status from hook event name (like local events)
                let (derived_status, attention_type) =
                    event::derive_status_from_hook_name(&hook_name);

                let hook_event = event::HookEvent {
                    event: hook_name,
                    status: derived_status,
                    attention_type,
                    pane_id: remote_event.sprite_id.clone(),
                    project: remote_event
                        .event
                        .transcript_path
                        .as_ref()
                        .and_then(|p| std::path::Path::new(p).file_name())
                        .and_then(|n| n.to_str())
                        .map(String::from)
                        .unwrap_or_else(|| "sprite".to_string()),
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
                    // Claude Code 2.1.x fields (not yet available from sprites)
                    context_window: None,
                    agent_type: None,
                    permission_mode: None,
                    cwd: None,
                    transcript_path: remote_event.event.transcript_path,
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

        Some((forwarder_handle, converter_handle, status_handle))
    } else {
        None
    };

    // Load configuration from file
    let config = config::RehoboamConfig::load();
    tracing::info!(
        "Loaded config: sprites.enabled = {}, reconciliation.enabled = {}",
        config.sprites.enabled,
        config.reconciliation.enabled
    );

    // Create SpritesClient if token is provided
    let sprites_client = cli.sprites_token.as_ref().map(|token| {
        tracing::info!("Creating SpritesClient");
        sprites::SpritesClient::new(token)
    });

    // Run TUI
    let result = run_tui(
        event_tx,
        event_rx,
        cli.debug,
        cli.tick_rate,
        cli.frame_rate,
        sprites_client,
        &config,
    )
    .await;

    // Cleanup
    socket_handle.abort();

    // Cleanup sprite handles if enabled
    if let Some((forwarder_handle, converter_handle, status_handle)) = sprite_handle {
        forwarder_handle.abort();
        converter_handle.abort();
        status_handle.abort();
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
    sprites_client: Option<sprites::SpritesClient>,
    config: &config::RehoboamConfig,
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

    // Create app state with sprites client, event channel, and reconciliation config
    let mut app = App::new(
        debug_mode,
        sprites_client,
        Some(event_tx.clone()),
        &config.reconciliation,
    );

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
            () = tokio::time::sleep(tick_duration) => {
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
