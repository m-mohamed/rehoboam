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
use tracing::{debug, error, info, warn};

/// Connection status event for sprite
#[derive(Debug, Clone)]
pub enum ConnectionStatus {
    /// Sprite connected
    Connected { sprite_id: String, addr: SocketAddr },
    /// Sprite disconnected
    Disconnected { sprite_id: String, reason: String },
    /// Heartbeat timeout (sprite may be stale)
    HeartbeatMissed { sprite_id: String },
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
    connections: Arc<RwLock<HashMap<String, ConnectionInfo>>>,
}

/// Connection state for a sprite
#[derive(Debug, Clone)]
pub struct ConnectionInfo {
    /// Remote address
    pub addr: SocketAddr,

    /// When connected
    pub connected_at: std::time::Instant,

    /// Last activity timestamp
    pub last_seen: std::time::Instant,

    /// Number of events received
    pub event_count: u64,
}

impl HookEventForwarder {
    /// Create a new forwarder
    pub fn new(event_tx: mpsc::Sender<RemoteHookEvent>) -> Self {
        Self {
            event_tx,
            status_tx: None,
            connections: Arc::new(RwLock::new(HashMap::new())),
        }
    }

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

    /// Send a status update
    async fn send_status(&self, status: ConnectionStatus) {
        if let Some(tx) = &self.status_tx {
            let _ = tx.send(status).await;
        }
    }

    /// Get connection info for all sprites
    pub async fn connection_info(&self) -> HashMap<String, ConnectionInfo> {
        let conns = self.connections.read().await;
        conns.clone()
    }

    /// Get count of active connections
    pub async fn connection_count(&self) -> usize {
        let conns = self.connections.read().await;
        conns.len()
    }

    /// Start listening for WebSocket connections
    pub async fn listen(self, port: u16) -> Result<()> {
        let addr = format!("0.0.0.0:{}", port);
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

                    tokio::spawn(async move {
                        if let Err(e) =
                            Self::handle_connection(stream, addr, event_tx, status_tx, connections)
                                .await
                        {
                            error!("WebSocket connection error from {}: {}", addr, e);
                        }
                    });
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
                                        addr,
                                        connected_at: now,
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

                            debug!(
                                "Received hook event from {}: {:?}",
                                event.sprite_id, event.event.hook_event_name
                            );

                            // Forward to main event loop
                            if let Err(e) = event_tx.send(event).await {
                                error!("Failed to forward event: {}", e);
                                disconnect_reason = format!("Channel error: {}", e);
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
                                disconnect_reason = format!("Channel error: {}", e);
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
                    disconnect_reason = format!("WebSocket error: {}", e);
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

    /// Check if a sprite is connected
    pub async fn is_connected(&self, sprite_id: &str) -> bool {
        let conns = self.connections.read().await;
        conns.contains_key(sprite_id)
    }

    /// Get list of connected sprite IDs
    pub async fn connected_sprites(&self) -> Vec<String> {
        let conns = self.connections.read().await;
        conns.keys().cloned().collect()
    }
}

/// Start the hook event forwarder in a background task
pub fn spawn_forwarder(
    port: u16,
) -> (mpsc::Receiver<RemoteHookEvent>, tokio::task::JoinHandle<()>) {
    let (tx, rx) = mpsc::channel(100);
    let forwarder = HookEventForwarder::new(tx);

    let handle = tokio::spawn(async move {
        if let Err(e) = forwarder.listen(port).await {
            error!("Hook event forwarder error: {}", e);
        }
    });

    (rx, handle)
}

/// Start the hook event forwarder with status channel
pub fn spawn_forwarder_with_status(
    port: u16,
) -> (
    mpsc::Receiver<RemoteHookEvent>,
    mpsc::Receiver<ConnectionStatus>,
    tokio::task::JoinHandle<()>,
) {
    let (event_tx, event_rx) = mpsc::channel(100);
    let (status_tx, status_rx) = mpsc::channel(50);
    let forwarder = HookEventForwarder::with_status_channel(event_tx, status_tx);

    let handle = tokio::spawn(async move {
        if let Err(e) = forwarder.listen(port).await {
            error!("Hook event forwarder error: {}", e);
        }
    });

    (event_rx, status_rx, handle)
}
