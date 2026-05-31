//! STunT: A TUI for managing SSH tunnel connections.

mod app;
mod config;
mod demo;
mod event;
mod tunnel;
mod ui;

use std::io;
use std::time::Duration;

use clap::Parser;
use crossterm::event::{KeyCode, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use app::{App, AppMode, Message};
use config::TunnelEntry;
use demo::DemoUiEvent;
use event::AppEvent;

/// STunT — Stupid Tunnel Tricks: A TUI for managing SSH tunnel connections.
#[derive(Parser)]
#[command(name = "stunt", version, about)]
struct Cli {
    /// Launch in demo mode with simulated tunnels (no real connections).
    #[arg(long)]
    demo: bool,
}

fn main() -> anyhow::Result<()> {
    // Build the runtime manually (rather than `#[tokio::main]`) so we control
    // its shutdown. The app intentionally leaves tunnel processes running on
    // quit, which keeps their stdout/stderr pipes open; the detached reader
    // tasks and the crossterm console reader are therefore parked on blocking
    // I/O that never completes. If we let the runtime drop normally at the end
    // of `main`, that drop blocks until those parked threads unwind (on Windows
    // this requires a console event — hence the hang until Ctrl-C). Instead we
    // run the app to completion, then force the runtime down with a short
    // timeout so the process exits promptly regardless of parked I/O.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let result = runtime.block_on(run());

    // Force the runtime to stop within a bounded time, abandoning any tasks
    // still parked on blocking reads (e.g. the console EventStream and the
    // tunnel stdout/stderr readers on the still-running detached ssh process).
    runtime.shutdown_timeout(Duration::from_millis(100));

    result
}

/// Async application entry point. Runs the full TUI lifecycle and restores the
/// terminal before returning.
async fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Load configuration (or build demo fixtures)
    let cfg = if cli.demo {
        config::Config {
            entries: demo::demo_entries(),
        }
    } else {
        // Check that ssh is available on PATH (only in normal mode)
        tunnel::check_ssh_available()?;
        config::load()?
    };

    // Start the event loop (terminal + tunnel event channels)
    let (tunnel_tx, mut app_rx) = event::start_event_loop();

    // Create the demo UI event channel when in demo mode.
    let (demo_ui_tx, demo_ui_rx) = if cli.demo {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<DemoUiEvent>();
        (Some(tx), Some(rx))
    } else {
        (None, None)
    };

    // Create the application
    let mut app = App::new(cfg.clone(), tunnel_tx.clone(), cli.demo, demo_ui_rx);

    // Start demo simulation and dialog tour if in demo mode.
    let demo_cancel = if cli.demo {
        let cancel = demo::start_demo(&cfg.entries, tunnel_tx);
        if let Some(tx) = demo_ui_tx {
            demo::start_demo_tour(&cfg.entries, tx, cancel.clone());
        }
        Some(cancel)
    } else {
        None
    };

    if !cli.demo {
        // Warn if kubectl is unavailable and K8s entries are configured
        let has_k8s_entries = cfg.entries.iter().any(|e| matches!(e, TunnelEntry::K8s(_)));
        if has_k8s_entries && !tunnel::check_kubectl_available() {
            app.set_kubectl_warning(
                "kubectl not found on PATH — K8s tunnels unavailable. Install kubectl to use K8s entries."
                    .to_string(),
            );
        }

        // Warn if sshuttle is unavailable and sshuttle entries are configured
        let has_sshuttle_entries = cfg
            .entries
            .iter()
            .any(|e| matches!(e, TunnelEntry::Sshuttle(_)));
        if has_sshuttle_entries && !tunnel::check_sshuttle_available() {
            app.set_sshuttle_warning(
                "sshuttle not found on PATH — sshuttle tunnels unavailable. Install sshuttle to use these entries."
                    .to_string(),
            );
        }
    }

    // Initialize terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    // Run the main event loop, capturing its result so the terminal is always
    // restored even if rendering or event handling fails partway through.
    let loop_result = run_event_loop(&mut terminal, &mut app, &mut app_rx).await;

    // Shutdown: cancel demo tasks or terminate all active supervisors
    if let Some(cancel) = demo_cancel {
        cancel.cancel();
    }
    app.shutdown();

    // Restore terminal (always, regardless of loop_result)
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    loop_result
}

/// Drive the main event loop until the user quits or an error occurs.
///
/// Kept separate from `run` so the terminal can be unconditionally restored by
/// the caller regardless of how this function returns.
async fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    app_rx: &mut tokio::sync::mpsc::UnboundedReceiver<AppEvent>,
) -> anyhow::Result<()> {
    while app.running {
        // Render
        terminal.draw(|frame| {
            ui::draw(frame, app);
        })?;

        // Drain all pending demo UI events before processing keyboard input.
        // Collect first to avoid holding the borrow on demo_ui_rx while
        // dispatching into other App methods.
        let demo_events: Vec<DemoUiEvent> = if let Some(rx) = app.demo_ui_rx.as_mut() {
            std::iter::from_fn(|| rx.try_recv().ok()).collect()
        } else {
            vec![]
        };
        for demo_event in demo_events {
            match demo_event {
                DemoUiEvent::OpenTypeSelector => app.demo_open_type_selector(),
                DemoUiEvent::HighlightType(idx) => app.demo_highlight_type(idx),
                DemoUiEvent::SelectTunnelType(t) => app.demo_select_tunnel_type(t),
                DemoUiEvent::SelectEntry(idx) => app.demo_select_entry(idx),
                DemoUiEvent::OpenEditForm(id) => app.demo_open_edit_form(id),
                DemoUiEvent::CloseDialog => app.demo_close_dialog(),
            }
        }

        // Wait for next event. In demo mode we also wake on a short tick so
        // that DemoUiEvents (sent by the tour task) are picked up and rendered
        // promptly even when no keyboard or tunnel events are arriving.
        let maybe_event = if app.demo_mode {
            tokio::select! {
                ev = app_rx.recv() => ev,
                _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => None,
            }
        } else {
            app_rx.recv().await
        };

        if let Some(event) = maybe_event {
            match event {
                AppEvent::Key(key) => {
                    // In demo mode all user input is silently ignored except
                    // quit — the tour task drives the UI.
                    if app.demo_mode {
                        let is_quit = key.code == KeyCode::Char('q')
                            || (key.code == KeyCode::Char('c')
                                && key.modifiers.contains(KeyModifiers::CONTROL));
                        if is_quit {
                            app.update(Message::Quit);
                        }
                    } else {
                        let msg = match app.mode {
                            AppMode::Normal => match key.code {
                                KeyCode::Char('q') => Some(Message::Quit),
                                KeyCode::Up | KeyCode::Char('k') => Some(Message::NavigateUp),
                                KeyCode::Down | KeyCode::Char('j') => Some(Message::NavigateDown),
                                KeyCode::Enter => Some(Message::ToggleConnect),
                                KeyCode::Char('n') => Some(Message::NewEntry),
                                KeyCode::Char('e') => Some(Message::EditEntry),
                                KeyCode::Char('d') => Some(Message::DeleteEntry),
                                KeyCode::PageUp => Some(Message::LogScrollUp),
                                KeyCode::PageDown => Some(Message::LogScrollDown),
                                KeyCode::End => Some(Message::LogScrollToBottom),
                                KeyCode::Char('c')
                                    if key.modifiers.contains(KeyModifiers::CONTROL) =>
                                {
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
                                KeyCode::Char('t')
                                    if key.modifiers.contains(KeyModifiers::CONTROL) =>
                                {
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
                                KeyCode::Char('a')
                                    if key.modifiers.contains(KeyModifiers::CONTROL) =>
                                {
                                    Some(Message::FormAddForward)
                                }
                                KeyCode::Char('d')
                                    if key.modifiers.contains(KeyModifiers::CONTROL) =>
                                {
                                    Some(Message::FormDeleteForward)
                                }
                                KeyCode::Char('t')
                                    if key.modifiers.contains(KeyModifiers::CONTROL) =>
                                {
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

    Ok(())
}
