//! Terminal event handling (key, mouse, resize).
//!
//! Defines the unified `AppEvent` type and provides an async event reader
//! that bridges crossterm terminal events into a tokio mpsc channel.

use crossterm::event::{Event, EventStream, KeyEvent, KeyEventKind};
use futures::StreamExt;
use tokio::sync::mpsc;

use crate::tunnel::TunnelEvent;

/// Unified application event type.
///
/// Multiplexes terminal input events and tunnel lifecycle events into a
/// single stream for the main app loop.
#[derive(Debug)]
pub enum AppEvent {
    /// A key press event from the terminal.
    Key(KeyEvent),
    /// The terminal was resized (width, height).
    #[allow(dead_code)]
    Resize(u16, u16),
    /// A tunnel supervision task reported a state change.
    Tunnel(TunnelEvent),
}

/// Spawn a task that reads crossterm terminal events and forwards them
/// as `AppEvent` values to the provided sender.
///
/// Returns the sender for tunnel events (to be shared with supervisors)
/// and the receiver for the unified app event stream.
pub fn start_event_loop() -> (
    mpsc::UnboundedSender<TunnelEvent>,
    mpsc::UnboundedReceiver<AppEvent>,
) {
    let (app_tx, app_rx) = mpsc::unbounded_channel::<AppEvent>();
    let (tunnel_tx, mut tunnel_rx) = mpsc::unbounded_channel::<TunnelEvent>();

    let app_tx_term = app_tx.clone();
    let app_tx_tunnel = app_tx;

    // Terminal event reader task
    tokio::spawn(async move {
        let mut reader = EventStream::new();
        loop {
            match reader.next().await {
                Some(Ok(Event::Key(key))) if key.kind == KeyEventKind::Press => {
                    if app_tx_term.send(AppEvent::Key(key)).is_err() {
                        break;
                    }
                }
                Some(Ok(Event::Key(_))) => {
                    // Ignore key release and repeat events (Windows emits both)
                }
                Some(Ok(Event::Resize(w, h))) => {
                    if app_tx_term.send(AppEvent::Resize(w, h)).is_err() {
                        break;
                    }
                }
                Some(Ok(_)) => {
                    // Ignore mouse and other events
                }
                Some(Err(_)) => {
                    break;
                }
                None => {
                    break;
                }
            }
        }
    });

    // Tunnel event forwarder task
    tokio::spawn(async move {
        while let Some(event) = tunnel_rx.recv().await {
            if app_tx_tunnel.send(AppEvent::Tunnel(event)).is_err() {
                break;
            }
        }
    });

    (tunnel_tx, app_rx)
}
