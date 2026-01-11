use super::Event;
use crossterm::event::{self, Event as CrosstermEvent};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// Listen for keyboard input events with graceful shutdown support
pub async fn listen(tx: mpsc::Sender<Event>, cancel: CancellationToken) {
    loop {
        tokio::select! {
            // Check for cancellation signal
            () = cancel.cancelled() => {
                tracing::debug!("Input listener cancelled");
                break;
            }
            // Poll for input with timeout
            () = tokio::time::sleep(Duration::from_millis(100)) => {
                // Use non-blocking poll (Duration::ZERO) since we're already in a timeout
                if event::poll(Duration::ZERO).unwrap_or(false) {
                    if let Ok(CrosstermEvent::Key(key)) = event::read() {
                        if tx.send(Event::Key(key)).await.is_err() {
                            // Channel closed, exit
                            break;
                        }
                    }
                }
            }
        }
    }
}
