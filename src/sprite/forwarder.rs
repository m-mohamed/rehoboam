//! Hook event forwarder
//!
//! WebSocket server that receives hook events from remote sprites
//! and forwards them to the main event loop. Includes connection
//! status tracking and heartbeat monitoring.

use color_eyre::eyre::{eyre, Result};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, RwLock};
use tokio_tungstenite::{accept_async, tungstenite::Message};
use tracing::{debug, error, info, info_span, warn, Instrument};

/// Connection status event for sprite
#[derive(Debug, Clone)]
pub enum ConnectionStatus {
    /// Sprite connected
    Connected { sprite_id: String, addr: SocketAddr },
    /// Sprite disconnected
    Disconnected { sprite_id: String, reason: String },
}

/// Remote hook event from a sprite
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteHookEvent {
    /// Sprite identifier
    pub sprite_id: String,

    /// Original hook event data
    pub event: HookEventData,

    /// Timestamp when event was sent
    #[serde(default)]
    pub timestamp: Option<i64>,

    /// W3C Trace Context for distributed tracing (OTEL)
    /// Format: "00-{trace_id}-{span_id}-{flags}"
    #[serde(default)]
    pub trace_context: Option<String>,
}

/// Hook event data matching Claude Code's hook format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookEventData {
    /// Session ID
    #[serde(default)]
    pub session_id: Option<String>,

    /// Hook event name (PreToolUse, PostToolUse, etc.)
    #[serde(default)]
    pub hook_event_name: Option<String>,

    /// Tool name (for tool events)
    #[serde(default)]
    pub tool_name: Option<String>,

    /// Tool input (for tool events)
    #[serde(default)]
    pub tool_input: Option<serde_json::Value>,

    /// Transcript path
    #[serde(default)]
    pub transcript_path: Option<String>,
}

/// Manages WebSocket connections from sprites
pub struct HookEventForwarder {
    /// Channel to send events to main loop
    event_tx: mpsc::Sender<RemoteHookEvent>,

    /// Channel to send connection status updates
    status_tx: Option<mpsc::Sender<ConnectionStatus>>,

    /// Active connections (sprite_id -> connection metadata)
    pub(crate) connections: Arc<RwLock<HashMap<String, ConnectionInfo>>>,
}

/// Connection state for a sprite
#[derive(Debug, Clone)]
pub(crate) struct ConnectionInfo {
    /// Last activity timestamp
    last_seen: std::time::Instant,

    /// Number of events received
    event_count: u64,
}

impl HookEventForwarder {
    /// Create a new forwarder with status channel
    pub fn with_status_channel(
        event_tx: mpsc::Sender<RemoteHookEvent>,
        status_tx: mpsc::Sender<ConnectionStatus>,
    ) -> Self {
        Self {
            event_tx,
            status_tx: Some(status_tx),
            connections: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Spawn a periodic sweep task that removes stale connection entries.
    ///
    /// If a sprite disconnects without sending a Close frame (e.g., network drop),
    /// the connection handler's read loop may hang. This sweep catches entries
    /// whose `last_seen` is older than `stale_secs` and emits disconnect events.
    pub fn spawn_stale_reaper(
        connections: Arc<RwLock<HashMap<String, ConnectionInfo>>>,
        status_tx: Option<mpsc::Sender<ConnectionStatus>>,
        stale_secs: u64,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let interval = tokio::time::Duration::from_secs(60);
            let stale_threshold = std::time::Duration::from_secs(stale_secs);
            loop {
                tokio::time::sleep(interval).await;
                let now = std::time::Instant::now();
                let mut conns = connections.write().await;
                let stale_ids: Vec<String> = conns
                    .iter()
                    .filter(|(_, info)| now.duration_since(info.last_seen) > stale_threshold)
                    .map(|(id, _)| id.clone())
                    .collect();
                for id in stale_ids {
                    conns.remove(&id);
                    warn!("Reaped stale sprite connection: {}", id);
                    if let Some(tx) = &status_tx {
                        let _ = tx
                            .send(ConnectionStatus::Disconnected {
                                sprite_id: id,
                                reason: "Stale connection reaped".to_string(),
                            })
                            .await;
                    }
                }
            }
        })
    }

    /// Start listening for WebSocket connections
    pub async fn listen(self, port: u16) -> Result<()> {
        let addr = format!("0.0.0.0:{port}");
        let listener = TcpListener::bind(&addr)
            .await
            .map_err(|e| eyre!("Failed to bind WebSocket server: {}", e))?;

        info!("Hook event forwarder listening on {}", addr);

        loop {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    let event_tx = self.event_tx.clone();
                    let status_tx = self.status_tx.clone();
                    let connections = self.connections.clone();

                    // Create a span for the connection with OTEL context
                    let connection_span = info_span!(
                        "sprite_connection",
                        sprite.addr = %addr,
                        otel.kind = "server"
                    );

                    tokio::spawn(
                        async move {
                            if let Err(e) = Self::handle_connection(
                                stream,
                                addr,
                                event_tx,
                                status_tx,
                                connections,
                            )
                            .await
                            {
                                error!("WebSocket connection error from {}: {}", addr, e);
                            }
                        }
                        .instrument(connection_span),
                    );
                }
                Err(e) => {
                    error!("Failed to accept connection: {}", e);
                }
            }
        }
    }

    /// Handle a single WebSocket connection
    async fn handle_connection(
        stream: TcpStream,
        addr: SocketAddr,
        event_tx: mpsc::Sender<RemoteHookEvent>,
        status_tx: Option<mpsc::Sender<ConnectionStatus>>,
        connections: Arc<RwLock<HashMap<String, ConnectionInfo>>>,
    ) -> Result<()> {
        debug!("New WebSocket connection from {}", addr);

        let ws_stream = accept_async(stream)
            .await
            .map_err(|e| eyre!("WebSocket handshake failed: {}", e))?;

        let (mut ws_write, mut ws_read) = ws_stream.split();
        let mut sprite_id: Option<String> = None;
        let mut disconnect_reason = "Connection closed".to_string();

        while let Some(msg) = ws_read.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    match serde_json::from_str::<RemoteHookEvent>(&text) {
                        Ok(event) => {
                            let now = std::time::Instant::now();

                            // Track connection by sprite_id
                            if sprite_id.is_none() {
                                sprite_id = Some(event.sprite_id.clone());
                                let mut conns = connections.write().await;
                                conns.insert(
                                    event.sprite_id.clone(),
                                    ConnectionInfo {
                                        last_seen: now,
                                        event_count: 1,
                                    },
                                );
                                info!("Sprite {} connected from {}", event.sprite_id, addr);

                                // Send connected status
                                if let Some(tx) = &status_tx {
                                    let _ = tx
                                        .send(ConnectionStatus::Connected {
                                            sprite_id: event.sprite_id.clone(),
                                            addr,
                                        })
                                        .await;
                                }
                            } else {
                                // Update last_seen and event_count
                                let mut conns = connections.write().await;
                                if let Some(info) = conns.get_mut(&event.sprite_id) {
                                    info.last_seen = now;
                                    info.event_count += 1;
                                }
                            }

                            // Create span for the event with trace context
                            let event_span = info_span!(
                                "sprite_hook_event",
                                sprite.id = %event.sprite_id,
                                hook.name = ?event.event.hook_event_name,
                                tool.name = ?event.event.tool_name,
                                otel.kind = "internal",
                                // Include trace context if provided by sprite
                                trace.parent = ?event.trace_context,
                            );

                            let _guard = event_span.enter();

                            debug!(
                                "Received hook event from {}: {:?}",
                                event.sprite_id, event.event.hook_event_name
                            );

                            // Forward to main event loop
                            if let Err(e) = event_tx.send(event).await {
                                error!("Failed to forward event: {}", e);
                                disconnect_reason = format!("Channel error: {e}");
                                break;
                            }
                        }
                        Err(e) => {
                            warn!("Invalid hook event JSON from {}: {}", addr, e);
                        }
                    }
                }
                Ok(Message::Binary(data)) => {
                    // Try to parse binary as JSON too
                    if let Ok(text) = String::from_utf8(data.to_vec()) {
                        if let Ok(event) = serde_json::from_str::<RemoteHookEvent>(&text) {
                            // Update last_seen
                            if let Some(id) = &sprite_id {
                                let mut conns = connections.write().await;
                                if let Some(info) = conns.get_mut(id) {
                                    info.last_seen = std::time::Instant::now();
                                    info.event_count += 1;
                                }
                            }

                            if let Err(e) = event_tx.send(event).await {
                                error!("Failed to forward event: {}", e);
                                disconnect_reason = format!("Channel error: {e}");
                                break;
                            }
                        }
                    }
                }
                Ok(Message::Ping(data)) => {
                    let _ = ws_write.send(Message::Pong(data)).await;
                    // Update last_seen on ping
                    if let Some(id) = &sprite_id {
                        let mut conns = connections.write().await;
                        if let Some(info) = conns.get_mut(id) {
                            info.last_seen = std::time::Instant::now();
                        }
                    }
                }
                Ok(Message::Close(_)) => {
                    debug!("WebSocket closed by client {}", addr);
                    disconnect_reason = "Client closed connection".to_string();
                    break;
                }
                Err(e) => {
                    error!("WebSocket error from {}: {}", addr, e);
                    disconnect_reason = format!("WebSocket error: {e}");
                    break;
                }
                _ => {}
            }
        }

        // Clean up connection tracking and send status
        if let Some(id) = sprite_id {
            let mut conns = connections.write().await;
            conns.remove(&id);
            info!("Sprite {} disconnected: {}", id, disconnect_reason);

            // Send disconnected status
            if let Some(tx) = &status_tx {
                let _ = tx
                    .send(ConnectionStatus::Disconnected {
                        sprite_id: id,
                        reason: disconnect_reason,
                    })
                    .await;
            }
        }

        Ok(())
    }
}

/// Start the hook event forwarder with status channel and stale connection reaper
pub fn spawn_forwarder_with_status(
    port: u16,
) -> (
    mpsc::Receiver<RemoteHookEvent>,
    mpsc::Receiver<ConnectionStatus>,
    tokio::task::JoinHandle<()>,
    tokio::task::JoinHandle<()>,
) {
    let (event_tx, event_rx) = mpsc::channel(100);
    let (status_tx, status_rx) = mpsc::channel(50);
    let forwarder = HookEventForwarder::with_status_channel(event_tx, status_tx.clone());

    // Start stale connection reaper (every 60s, reap connections idle >120s)
    let reaper_handle = HookEventForwarder::spawn_stale_reaper(
        forwarder.connections.clone(),
        Some(status_tx),
        120,
    );

    let handle = tokio::spawn(async move {
        if let Err(e) = forwarder.listen(port).await {
            error!("Hook event forwarder error: {}", e);
        }
    });

    (event_rx, status_rx, handle, reaper_handle)
}
