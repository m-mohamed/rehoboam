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
    /// Enable remote sprite support (run Claude Code in sandboxed VMs)
    #[arg(long, default_value_t = false, global = true)]
    pub enable_sprites: bool,

    /// Sprites API token (required if --enable-sprites)
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
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Process Claude Code hook JSON from stdin (v1.0+)
    ///
    /// Reads hook event JSON piped from Claude Code hooks, parses all fields,
    /// and sends enriched event to the TUI via Unix socket.
    Hook {
        /// Send desktop notification (for attention and stop events)
        #[arg(short = 'N', long, default_value_t = false)]
        notify: bool,
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
}

/// Generate shell completions and print to stdout
pub fn print_completions(shell: Shell) {
    let mut cmd = Cli::command();
    generate(shell, &mut cmd, "rehoboam", &mut io::stdout());
}

/// Runtime context derived from CLI arguments
///
/// Provides a clean interface for accessing configuration throughout the app.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct Context {
    /// Unix socket path for IPC
    pub socket: PathBuf,
    /// Log level string
    pub log_level: String,
    /// Debug mode enabled
    pub debug: bool,
}

impl From<&Cli> for Context {
    fn from(cli: &Cli) -> Self {
        Self {
            socket: cli.socket.clone(),
            log_level: cli.log_level.clone(),
            debug: cli.debug,
        }
    }
}
