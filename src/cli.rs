//! CLI argument parsing and runtime context
//!
//! Uses clap for argument parsing with derive macros.
//! Provides a Context struct for runtime configuration.

use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};
use std::io;
use std::path::PathBuf;

/// Get default socket path, preferring XDG_RUNTIME_DIR on Linux
fn default_socket_path() -> PathBuf {
    // Try XDG_RUNTIME_DIR first (Linux best practice)
    if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
        return PathBuf::from(runtime_dir).join("rehoboam.sock");
    }
    // Fall back to /tmp (macOS and fallback)
    PathBuf::from("/tmp/rehoboam.sock")
}

/// Real-time observability TUI for Claude Code agents - tracks all, predicts needs
#[derive(Parser, Debug)]
#[command(name = "rehoboam")]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Socket path for receiving hook events (default: $XDG_RUNTIME_DIR/rehoboam.sock or /tmp/rehoboam.sock)
    #[arg(
        short,
        long,
        env = "REHOBOAM_SOCKET",
        default_value_os_t = default_socket_path(),
        global = true
    )]
    pub socket: PathBuf,

    /// Log level (trace, debug, info, warn, error)
    #[arg(short, long, env = "RUST_LOG", default_value = "info", global = true)]
    pub log_level: String,

    /// Run in debug mode (shows event log)
    #[arg(short, long, default_value_t = false, global = true)]
    pub debug: bool,

    /// Tick rate in ticks per second (default: 1.0)
    #[arg(short = 't', long, default_value_t = 1.0, global = true)]
    pub tick_rate: f64,

    /// Frame rate in frames per second (default: 30.0)
    #[arg(short = 'F', long, default_value_t = 30.0, global = true)]
    pub frame_rate: f64,

    /// Install binary to ~/.local/bin/
    #[arg(long, default_value_t = false)]
    pub install: bool,

    // Sprites integration options
    /// Disable remote sprite support (sprites auto-enable when SPRITES_TOKEN is set)
    #[arg(long, default_value_t = false, global = true)]
    pub no_sprites: bool,

    /// Sprites API token (enables sprite support when set)
    #[arg(long, env = "SPRITES_TOKEN", global = true)]
    pub sprites_token: Option<String>,

    /// WebSocket port for receiving hook events from remote sprites
    #[arg(
        long,
        env = "REHOBOAM_SPRITE_PORT",
        default_value_t = 9876,
        global = true
    )]
    pub sprite_ws_port: u16,

    // OpenTelemetry integration
    /// Enable OpenTelemetry export for distributed tracing
    ///
    /// Traces are exported to the OTLP endpoint (gRPC port 4317).
    /// Use with Jaeger, Grafana Tempo, or any OTLP-compatible collector.
    /// Example: --otel-endpoint http://localhost:4317
    #[arg(long, env = "REHOBOAM_OTEL_ENDPOINT", global = true)]
    pub otel_endpoint: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Process Claude Code hook JSON from stdin (v1.0+)
    ///
    /// Reads hook event JSON piped from Claude Code hooks, parses all fields,
    /// and sends enriched event to the TUI via Unix socket.
    Hook {
        /// Disable desktop notifications (notifications are ON by default)
        #[arg(long, default_value_t = false)]
        no_notify: bool,

        /// Output additionalContext for loop mode (Claude Code 2.1.x)
        ///
        /// When enabled and .rehoboam/ directory exists, outputs JSON with
        /// additionalContext field that Claude Code injects into the conversation.
        /// Only applies to PreToolUse and PostToolUse hooks.
        #[arg(long, default_value_t = false)]
        inject_context: bool,
    },

    /// Install Claude Code hooks to a project
    Init {
        /// Project path (default: current directory)
        path: Option<PathBuf>,

        /// Discover and select from git repos interactively
        #[arg(long, default_value_t = false)]
        all: bool,

        /// List discovered git repositories
        #[arg(long, default_value_t = false)]
        list: bool,

        /// Force overwrite existing hooks (default: merge)
        #[arg(long, default_value_t = false)]
        force: bool,
    },

    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },

    /// Manage remote sprites (cloud VMs)
    Sprites {
        #[command(subcommand)]
        action: SpritesAction,
    },
}

#[derive(Subcommand, Debug)]
pub enum SpritesAction {
    /// List all sprites with status
    List,
    /// Show detailed info for a sprite
    Info {
        /// Sprite name
        name: String,
    },
    /// Destroy a sprite (frees all resources)
    Destroy {
        /// Sprite name
        name: String,
    },
    /// Destroy all sprites
    DestroyAll {
        /// Skip confirmation
        #[arg(long, short = 'y')]
        yes: bool,
    },
}

/// Generate shell completions and print to stdout
pub fn print_completions(shell: Shell) {
    let mut cmd = Cli::command();
    generate(shell, &mut cmd, "rehoboam", &mut io::stdout());
}
