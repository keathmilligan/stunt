//! tunnel-mgr: A TUI for managing SSH tunnel connections.

mod app;
mod config;
mod event;
mod tunnel;
mod ui;

use std::io;

use crossterm::event::{KeyCode, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use app::{App, AppMode, Message};
use config::TunnelEntry;
use event::AppEvent;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Check that ssh is available on PATH
    tunnel::check_ssh_available()?;

    // Load configuration
    let cfg = config::load()?;

    // Start the event loop (terminal + tunnel event channels)
    let (tunnel_tx, mut app_rx) = event::start_event_loop();

    // Create the application
    let mut app = App::new(cfg.clone(), tunnel_tx);

    // Warn if kubectl is unavailable and K8s entries are configured
    let has_k8s_entries = cfg.entries.iter().any(|e| matches!(e, TunnelEntry::K8s(_)));
    if has_k8s_entries && !tunnel::check_kubectl_available() {
        app.set_kubectl_warning(
            "kubectl not found on PATH — K8s tunnels unavailable. Install kubectl to use K8s entries."
                .to_string(),
        );
    }

    // Initialize terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    // Main event loop
    while app.running {
        // Render
        terminal.draw(|frame| {
            ui::draw(frame, &app);
        })?;

        // Wait for next event
        if let Some(event) = app_rx.recv().await {
            match event {
                AppEvent::Key(key) => {
                    let msg = match app.mode {
                        AppMode::Normal => match key.code {
                            KeyCode::Char('q') => Some(Message::Quit),
                            KeyCode::Up | KeyCode::Char('k') => Some(Message::NavigateUp),
                            KeyCode::Down | KeyCode::Char('j') => Some(Message::NavigateDown),
                            KeyCode::Enter => Some(Message::ToggleConnect),
                            KeyCode::Char('n') => Some(Message::NewEntry),
                            KeyCode::Char('e') => Some(Message::EditEntry),
                            KeyCode::Char('d') => Some(Message::DeleteEntry),
                            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                Some(Message::Quit)
                            }
                            _ => None,
                        },
                        AppMode::TypeSelect(_) => match key.code {
                            KeyCode::Esc => Some(Message::FormCancel),
                            KeyCode::Enter => Some(Message::FormSubmit),
                            KeyCode::Tab | KeyCode::Down | KeyCode::Char('j') => {
                                Some(Message::FormNextField)
                            }
                            KeyCode::BackTab | KeyCode::Up | KeyCode::Char('k') => {
                                Some(Message::FormPrevField)
                            }
                            KeyCode::Char('t') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                Some(Message::FormCycleForwardType)
                            }
                            _ => None,
                        },
                        AppMode::Form(_) => match key.code {
                            KeyCode::Esc => Some(Message::FormCancel),
                            KeyCode::Enter => Some(Message::FormSubmit),
                            KeyCode::Tab | KeyCode::Down => Some(Message::FormNextField),
                            KeyCode::BackTab | KeyCode::Up => Some(Message::FormPrevField),
                            KeyCode::Backspace => Some(Message::FormBackspace),
                            KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                Some(Message::FormAddForward)
                            }
                            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                Some(Message::FormDeleteForward)
                            }
                            KeyCode::Char('t') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                Some(Message::FormCycleForwardType)
                            }
                            KeyCode::Char(c) => Some(Message::FormInput(c)),
                            _ => None,
                        },
                    };

                    if let Some(msg) = msg {
                        app.update(msg);
                    }
                }
                AppEvent::Resize(_, _) => {
                    // Terminal will re-render on next loop iteration
                }
                AppEvent::Tunnel(tunnel_event) => {
                    app.update(Message::TunnelEvent(tunnel_event));
                }
            }
        }
    }

    // Shutdown: terminate all active supervisors
    app.shutdown();

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}
