use super::{Event, HookEvent};
use color_eyre::Result;
use std::os::unix::io::{FromRawFd, IntoRawFd};
use std::path::Path;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::{mpsc, Semaphore};
use tokio::time::{timeout, Duration};

/// Maximum concurrent connections to prevent resource exhaustion
const MAX_CONNECTIONS: usize = 100;

/// Socket receive buffer size (4KB is plenty for ~500 byte JSON messages)
const SOCKET_RECV_BUF: usize = 4096;

/// Listen for hook events on Unix socket
pub async fn listen(tx: mpsc::Sender<Event>, socket_path: &Path) -> Result<()> {
    // Remove existing socket file
    if socket_path.exists() {
        std::fs::remove_file(socket_path)?;
    }

    // Create socket with socket2 for buffer tuning
    let socket = socket2::Socket::new(socket2::Domain::UNIX, socket2::Type::STREAM, None)?;

    // Tune receive buffer (4KB is plenty for ~500 byte JSON messages)
    // OS may clamp to minimum, which is fine
    if let Err(e) = socket.set_recv_buffer_size(SOCKET_RECV_BUF) {
        tracing::debug!("Could not set recv buffer size: {}", e);
    }

    // Bind and listen
    socket.bind(&socket2::SockAddr::unix(socket_path)?)?;
    socket.listen(128)?; // backlog of 128 pending connections
    socket.set_nonblocking(true)?;

    // Convert to tokio UnixListener
    let std_listener: std::os::unix::net::UnixListener =
        unsafe { std::os::unix::net::UnixListener::from_raw_fd(socket.into_raw_fd()) };
    let listener = UnixListener::from_std(std_listener)?;

    tracing::info!("Listening on {:?}", socket_path);

    // Semaphore to limit concurrent connections
    let semaphore = Arc::new(Semaphore::new(MAX_CONNECTIONS));

    // Backoff state for accept errors
    let mut backoff_ms: u64 = 0;
    const MAX_BACKOFF_MS: u64 = 5000;

    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                // Reset backoff on successful accept
                backoff_ms = 0;

                // Try to acquire permit (non-blocking check)
                let permit = match semaphore.clone().try_acquire_owned() {
                    Ok(permit) => permit,
                    Err(_) => {
                        tracing::warn!(
                            "Connection limit reached ({} max), dropping connection",
                            MAX_CONNECTIONS
                        );
                        continue;
                    }
                };

                let tx = tx.clone();
                tokio::spawn(async move {
                    // Permit is held until this task completes
                    let _permit = permit;

                    let reader = BufReader::new(stream);
                    let mut lines = reader.lines();

                    // Read with timeout - hooks send single-line JSON messages
                    // Use 2 second timeout to handle slow connections
                    let read_result = timeout(Duration::from_secs(2), lines.next_line()).await;

                    match read_result {
                        Ok(Ok(Some(line))) if !line.trim().is_empty() => {
                            match serde_json::from_str::<HookEvent>(&line) {
                                Ok(event) => {
                                    // Validate event before processing
                                    if let Err(e) = event.validate() {
                                        tracing::warn!("Invalid event: {} - {:?}", e, event);
                                    } else {
                                        tracing::debug!("Received event: {:?}", event);
                                        let _ = tx.send(Event::Hook(Box::new(event))).await;
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!("Failed to parse event: {} - {}", e, line);
                                }
                            }
                        }
                        Ok(Ok(Some(_))) => {} // Empty line, ignore
                        Ok(Ok(None)) => {}    // Stream closed
                        Ok(Err(e)) => {
                            tracing::warn!("Read error: {}", e);
                        }
                        Err(_) => {
                            tracing::debug!("Read timeout (connection may be stale)");
                        }
                    }
                });
            }
            Err(e) => {
                tracing::error!("Accept error: {}", e);

                // Exponential backoff to prevent CPU spin on persistent errors
                if backoff_ms == 0 {
                    backoff_ms = 100;
                } else {
                    backoff_ms = (backoff_ms * 2).min(MAX_BACKOFF_MS);
                }

                tracing::debug!("Backing off for {}ms", backoff_ms);
                tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
            }
        }
    }
}
