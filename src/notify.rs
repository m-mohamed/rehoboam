//! Cross-platform native desktop notifications
//!
//! Uses notify-rust for native notifications on macOS, Linux, and BSD.
//! No external dependencies like terminal-notifier required.

use notify_rust::Notification;

/// Send a desktop notification
///
/// On macOS, uses native NSUserNotification or UNUserNotification APIs.
/// On Linux, uses libnotify (freedesktop.org compliant).
///
/// Sound is currently ignored but kept for API compatibility.
pub fn send(title: &str, message: &str, _sound: Option<&str>) {
    // Spawn notification async - don't block on it
    let title = title.to_string();
    let message = message.to_string();

    std::thread::spawn(move || {
        let _ = Notification::new()
            .summary(&title)
            .body(&message)
            .timeout(5000) // 5 seconds
            .show();
    });
}
