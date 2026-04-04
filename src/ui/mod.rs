//! UI components and rendering.
//!
//! Implements a three-region layout (title bar, main area, status bar) where
//! the main area is split horizontally into a sidebar (tunnel list, 40%) and
//! a details panel (splash / forms, 60%). On terminals narrower than
//! `MIN_SPLIT_WIDTH` columns the details panel is omitted and the sidebar
//! occupies the full main area.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::app::{App, AppMode, EntryTypeSelection, FormEntryType, FormFocus, FormState};
use crate::config::{K8sEntry, ServerEntry, SshuttleEntry, TunnelEntry, TunnelForward};
use crate::tunnel::ConnectionState;

// ── Layout constants ────────────────────────────────────────────────────

/// Minimum terminal width (columns) to show the two-panel split.
/// Below this threshold only the sidebar is rendered.
const MIN_SPLIT_WIDTH: u16 = 80;

/// Fixed height in lines for every sidebar entry row (content only).
const SIDEBAR_ROW_HEIGHT: u16 = 6;

/// Total stride per entry including the 1-line divider that follows each row.
const SIDEBAR_ROW_STRIDE: u16 = SIDEBAR_ROW_HEIGHT + 1;

// ── Color palette ───────────────────────────────────────────────────────

const COLOR_CONNECTED: Color = Color::Green;
const COLOR_FAILED: Color = Color::Red;
const COLOR_TRANSIENT: Color = Color::Yellow;
const COLOR_DISCONNECTED: Color = Color::DarkGray;
const COLOR_SUSPENDED: Color = Color::Magenta;
const COLOR_FORWARD_LABEL: Color = Color::Cyan;
const COLOR_TITLE_BG: Color = Color::Blue;
const COLOR_STATUS_BG: Color = Color::DarkGray;
const COLOR_SELECTED_BG: Color = Color::Rgb(30, 40, 60);
const COLOR_K8S_LABEL: Color = Color::Rgb(100, 180, 255);
const COLOR_SSHUTTLE_LABEL: Color = Color::Rgb(180, 140, 255);

// ── Public entry point ──────────────────────────────────────────────────

/// Render the entire application UI.
pub fn draw(frame: &mut Frame, app: &App) {
    // Three-region vertical layout: title bar (1), main area (fill), status bar (1)
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .split(frame.area());

    render_title_bar(frame, rows[0], app);
    render_status_bar(frame, rows[2], app);

    let main_area = rows[1];

    if main_area.width >= MIN_SPLIT_WIDTH {
        // Two-panel split: sidebar (40%) | details panel (60%)
        let cols = Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(main_area);

        let sidebar_area = cols[0];
        let details_area = cols[1];

        render_sidebar(frame, sidebar_area, app);
        render_details_panel(frame, details_area, app);
    } else {
        // Narrow terminal: sidebar only
        render_sidebar(frame, main_area, app);
    }
}

// ── Title bar ───────────────────────────────────────────────────────────

/// Render the title bar with app name + version (left) and key hints (right).
fn render_title_bar(frame: &mut Frame, area: Rect, _app: &App) {
    let bg_style = Style::default()
        .fg(Color::White)
        .bg(COLOR_TITLE_BG)
        .add_modifier(Modifier::BOLD);

    let version = env!("CARGO_PKG_VERSION");
    let title = format!(" STunT v{version}");
    let hints = "[n]ew  [e]dit  [d]elete  [Enter] connect  [q]uit ";

    // Split into left (title) and right (hints) halves.
    // Both widgets use .style(bg_style) so the background fills every cell.
    let cols = Layout::horizontal([Constraint::Min(0), Constraint::Min(0)]).split(area);

    frame.render_widget(
        Paragraph::new(Line::from(Span::raw(title)))
            .style(bg_style)
            .alignment(Alignment::Left),
        cols[0],
    );
    frame.render_widget(
        Paragraph::new(Line::from(Span::raw(hints)))
            .style(bg_style)
            .alignment(Alignment::Right),
        cols[1],
    );
}

// ── Status bar ──────────────────────────────────────────────────────────

/// Render the status bar with connection summary and transient messages.
fn render_status_bar(frame: &mut Frame, area: Rect, app: &App) {
    let bg_style = Style::default().fg(Color::White).bg(COLOR_STATUS_BG);

    let total = app.entries.len();
    let connected = app.count_in_state(ConnectionState::Connected);
    let failed = app.count_in_state(ConnectionState::Failed);
    let suspended = app.count_in_state(ConnectionState::Suspended);

    let mut summary = format!(" {total} entries  {connected} connected  {failed} failed");
    if suspended > 0 {
        summary.push_str(&format!("  {suspended} suspended"));
    }

    let display_msg = app
        .active_status_message()
        .map(|s| s.to_string())
        .or_else(|| app.kubectl_warning.as_ref().map(|w| format!("⚠ {w}")))
        .or_else(|| app.sshuttle_warning.as_ref().map(|w| format!("⚠ {w}")));

    // Split into left (summary) and right (message). Both widgets carry the
    // background style so every cell in the bar is filled even when the text
    // is shorter than the terminal width.
    let cols = Layout::horizontal([Constraint::Min(0), Constraint::Min(0)]).split(area);

    frame.render_widget(
        Paragraph::new(Line::from(Span::raw(summary)))
            .style(bg_style)
            .alignment(Alignment::Left),
        cols[0],
    );

    if let Some(msg) = display_msg {
        let msg_style = Style::default().fg(COLOR_TRANSIENT).bg(COLOR_STATUS_BG);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(format!("{msg} "), msg_style)))
                .style(bg_style)
                .alignment(Alignment::Right),
            cols[1],
        );
    } else {
        // No message — render the right half as plain background fill.
        frame.render_widget(Paragraph::new(Line::from("")).style(bg_style), cols[1]);
    }
}

// ── Sidebar ─────────────────────────────────────────────────────────────

/// Render the sidebar: a scrollable list of fixed-height tunnel entry rows
/// with a right-side border separating it from the details panel.
fn render_sidebar(frame: &mut Frame, area: Rect, app: &App) {
    // Draw a block with a right border as the separator
    let block = Block::default()
        .borders(Borders::RIGHT)
        .border_style(Style::default().fg(Color::Rgb(50, 55, 65)));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if app.entries.is_empty() {
        let hint = Line::from(Span::styled(
            "No tunnels — press 'n' to add one",
            Style::default().fg(Color::DarkGray),
        ));
        let y = inner.y + inner.height / 2;
        if y < inner.y + inner.height {
            let hint_area = Rect {
                x: inner.x,
                y,
                width: inner.width,
                height: 1,
            };
            frame.render_widget(Paragraph::new(hint).alignment(Alignment::Center), hint_area);
        }
        return;
    }

    // Reserve 1 line at the top and 1 at the bottom for scroll indicators.
    // The list viewport sits between them.
    if inner.height < 3 {
        return; // too small to render anything useful
    }
    let indicator_top = Rect {
        x: inner.x,
        y: inner.y,
        width: inner.width,
        height: 1,
    };
    let list_area = Rect {
        x: inner.x,
        y: inner.y + 1,
        width: inner.width,
        height: inner.height - 2,
    };
    let indicator_bot = Rect {
        x: inner.x,
        y: inner.y + inner.height - 1,
        width: inner.width,
        height: 1,
    };

    let viewport_height = list_area.height as usize;
    let row_h = SIDEBAR_ROW_HEIGHT as usize;
    let stride = SIDEBAR_ROW_STRIDE as usize;
    let last_idx = app.entries.len().saturating_sub(1);

    // O(1) scroll: all rows are uniform height (stride includes the divider)
    let scroll_offset = compute_scroll_offset(
        app.selected,
        app.scroll_offset,
        app.entries.len(),
        viewport_height,
        stride,
    );

    // Determine whether there is content above / below the viewport
    let has_above = scroll_offset > 0;
    let total_content_height = app.entries.len() * stride;
    let has_below = scroll_offset + viewport_height < total_content_height;

    // Scroll indicators
    let indicator_style = Style::default().fg(Color::DarkGray);
    if has_above {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled("▲", indicator_style)))
                .alignment(Alignment::Center),
            indicator_top,
        );
    }
    if has_below {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled("▼", indicator_style)))
                .alignment(Alignment::Center),
            indicator_bot,
        );
    }

    let mut y_offset: u16 = 0;

    for (i, entry) in app.entries.iter().enumerate() {
        let top_of_row = i * stride;

        if top_of_row + stride <= scroll_offset {
            continue;
        }

        if y_offset >= list_area.height {
            break;
        }

        let is_selected = i == app.selected;
        let state = app.state_of(&entry.id());
        let available_h = (row_h as u16).min(list_area.height - y_offset);

        let row_area = Rect {
            x: list_area.x,
            y: list_area.y + y_offset,
            width: list_area.width,
            height: available_h,
        };

        match entry {
            TunnelEntry::Ssh(ssh) => render_ssh_entry_row(frame, row_area, ssh, state, is_selected),
            TunnelEntry::K8s(k8s) => render_k8s_entry_row(frame, row_area, k8s, state, is_selected),
            TunnelEntry::Sshuttle(s) => {
                render_sshuttle_entry_row(frame, row_area, s, state, is_selected)
            }
        }

        y_offset += row_h as u16;

        // Divider between entries (not after the last one).
        // Rendered across the full `area` width (including the border column) so
        // the rightmost character lands on the │ border, producing a ┤ join.
        if i < last_idx && y_offset < list_area.height {
            let divider_area = Rect {
                x: area.x,
                y: list_area.y + y_offset,
                width: area.width,
                height: 1,
            };
            // inner.width dashes + the t-join that sits on the border column
            let dashes = "─".repeat(inner.width as usize);
            let divider = Paragraph::new(Line::from(vec![
                Span::styled(dashes, Style::default().fg(Color::Rgb(50, 55, 65))),
                Span::styled("┤", Style::default().fg(Color::Rgb(50, 55, 65))),
            ]));
            frame.render_widget(divider, divider_area);
            y_offset += 1;
        }
    }
}

/// Compute the scroll offset (in lines) to keep the selected entry visible.
/// All rows have uniform height, so this is O(1).
fn compute_scroll_offset(
    selected: usize,
    current_offset: usize,
    entry_count: usize,
    viewport_height: usize,
    row_h: usize,
) -> usize {
    if entry_count == 0 || viewport_height == 0 {
        return 0;
    }

    let top_of_selected = selected * row_h;
    let bottom_of_selected = top_of_selected + row_h;

    let mut offset = current_offset;

    if top_of_selected < offset {
        offset = top_of_selected;
    }

    if bottom_of_selected > offset + viewport_height {
        offset = bottom_of_selected.saturating_sub(viewport_height);
    }

    offset
}

// ── Entry row helpers ────────────────────────────────────────────────────

/// Build a content line: 1-char colored strip + 1-char blank separator + content spans.
/// A wide trailing fill span ensures the selection background covers the full row width.
fn entry_line<'a>(strip_color: Color, bg: Color, content: Vec<Span<'a>>) -> Line<'a> {
    let mut spans = vec![
        Span::styled(" ", Style::default().bg(strip_color)),
        Span::styled(" ", Style::default().bg(bg)),
    ];
    spans.extend(content);
    // Fill remaining width so the bg color covers the entire row
    spans.push(Span::styled(" ".repeat(256), Style::default().bg(bg)));
    Line::from(spans)
}

/// Summarise a list of SSH local/remote forwards inline if ≤2, else as count.
/// `label` is "L", "R", or "D".
fn ssh_forward_summary(forwards: &[&TunnelForward], label: &str) -> String {
    match forwards.len() {
        0 => String::new(),
        1 => match forwards[0] {
            TunnelForward::Local {
                bind_port,
                remote_host,
                remote_port,
                ..
            } => {
                format!("{label}: {bind_port}→{remote_host}:{remote_port}")
            }
            TunnelForward::Remote {
                bind_port,
                remote_host,
                remote_port,
                ..
            } => {
                format!("{label}: {bind_port}←{remote_host}:{remote_port}")
            }
            TunnelForward::Dynamic { bind_port, .. } => {
                format!("{label}: {bind_port} (SOCKS)")
            }
        },
        2 => {
            let parts: Vec<String> = forwards
                .iter()
                .map(|f| match f {
                    TunnelForward::Local {
                        bind_port,
                        remote_host,
                        remote_port,
                        ..
                    } => {
                        format!("{bind_port}→{remote_host}:{remote_port}")
                    }
                    TunnelForward::Remote {
                        bind_port,
                        remote_host,
                        remote_port,
                        ..
                    } => {
                        format!("{bind_port}←{remote_host}:{remote_port}")
                    }
                    TunnelForward::Dynamic { bind_port, .. } => {
                        format!("{bind_port}(SOCKS)")
                    }
                })
                .collect();
            format!("{label}: {}", parts.join(", "))
        }
        n => format!("{n}{label}"),
    }
}

/// Summarise K8s port bindings: ≤2 inline, >2 as count.
fn k8s_bindings_summary(forwards: &[crate::config::K8sPortForward]) -> String {
    match forwards.len() {
        0 => String::new(),
        1 => format!("{}→:{}", forwards[0].local_port, forwards[0].remote_port),
        2 => format!(
            "{}→:{}, {}→:{}",
            forwards[0].local_port,
            forwards[0].remote_port,
            forwards[1].local_port,
            forwards[1].remote_port,
        ),
        n => format!("{n} bindings"),
    }
}

/// Auto-restart indicator span (or empty vec if not enabled).
fn auto_restart_spans(auto_restart: bool, bg: Color) -> Vec<Span<'static>> {
    if auto_restart {
        vec![Span::styled(
            " [R]",
            Style::default().fg(Color::DarkGray).bg(bg),
        )]
    } else {
        vec![]
    }
}

// ── SSH entry row ────────────────────────────────────────────────────────

/// Render an SSH entry row as exactly 6 lines.
fn render_ssh_entry_row(
    frame: &mut Frame,
    area: Rect,
    entry: &ServerEntry,
    state: ConnectionState,
    is_selected: bool,
) {
    let bg = if is_selected {
        COLOR_SELECTED_BG
    } else {
        Color::Reset
    };
    let strip_color = state_to_color(state);
    let state_label = format!("[{}]", state.label());
    let w = area.width;

    // Line 1: name, badge, state, auto-restart
    let mut l1_content = vec![
        Span::styled("[SSH] ", Style::default().fg(Color::DarkGray).bg(bg)),
        Span::styled(
            format!("{} ", entry.name),
            Style::default()
                .fg(Color::White)
                .bg(bg)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{state_label} "),
            Style::default().fg(strip_color).bg(bg),
        ),
    ];
    l1_content.extend(auto_restart_spans(entry.auto_restart, bg));

    // Line 2: host:port
    let l2_content = vec![Span::styled(
        format!("{}:{}", entry.host, entry.port),
        Style::default().fg(Color::White).bg(bg),
    )];

    // Line 3: identity
    let identity = match (&entry.user, &entry.identity_file) {
        (Some(u), Some(k)) => format!("user:{u}  key:{k}"),
        (Some(u), None) => format!("user:{u}"),
        (None, Some(k)) => format!("key:{k}"),
        (None, None) => String::new(),
    };
    let l3_content = vec![Span::styled(
        identity,
        Style::default().fg(Color::DarkGray).bg(bg),
    )];

    // Lines 4–6: forward summaries
    let locals: Vec<&TunnelForward> = entry
        .forwards
        .iter()
        .filter(|f| matches!(f, TunnelForward::Local { .. }))
        .collect();
    let remotes: Vec<&TunnelForward> = entry
        .forwards
        .iter()
        .filter(|f| matches!(f, TunnelForward::Remote { .. }))
        .collect();
    let dynamics: Vec<&TunnelForward> = entry
        .forwards
        .iter()
        .filter(|f| matches!(f, TunnelForward::Dynamic { .. }))
        .collect();

    let l4_text = ssh_forward_summary(&locals, "L");
    let l5_text = ssh_forward_summary(&remotes, "R");
    let l6_text = ssh_forward_summary(&dynamics, "D");

    let l4_content = vec![Span::styled(
        l4_text,
        Style::default().fg(COLOR_FORWARD_LABEL).bg(bg),
    )];
    let l5_content = vec![Span::styled(
        l5_text,
        Style::default().fg(COLOR_FORWARD_LABEL).bg(bg),
    )];
    let l6_content = vec![Span::styled(
        l6_text,
        Style::default().fg(COLOR_FORWARD_LABEL).bg(bg),
    )];

    let lines = vec![
        entry_line(strip_color, bg, l1_content),
        entry_line(strip_color, bg, l2_content),
        entry_line(strip_color, bg, l3_content),
        entry_line(strip_color, bg, l4_content),
        entry_line(strip_color, bg, l5_content),
        entry_line(strip_color, bg, l6_content),
    ];

    // Only render as many lines as fit in the area
    let visible: Vec<Line> = lines.into_iter().take(area.height as usize).collect();
    // Pad to full width
    let padded: Vec<Line> = visible.into_iter().map(|l| pad_line(l, w)).collect();

    frame.render_widget(Paragraph::new(padded), area);
}

// ── K8s entry row ────────────────────────────────────────────────────────

/// Render a K8s entry row as exactly 6 lines.
fn render_k8s_entry_row(
    frame: &mut Frame,
    area: Rect,
    entry: &K8sEntry,
    state: ConnectionState,
    is_selected: bool,
) {
    let bg = if is_selected {
        COLOR_SELECTED_BG
    } else {
        Color::Reset
    };
    let strip_color = state_to_color(state);
    let state_label = format!("[{}]", state.label());
    let w = area.width;

    // Line 1
    let mut l1_content = vec![
        Span::styled("[K8S] ", Style::default().fg(COLOR_K8S_LABEL).bg(bg)),
        Span::styled(
            format!("{} ", entry.name),
            Style::default()
                .fg(Color::White)
                .bg(bg)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{state_label} "),
            Style::default().fg(strip_color).bg(bg),
        ),
    ];
    l1_content.extend(auto_restart_spans(entry.auto_restart, bg));

    // Line 2: context
    let ctx = entry.context.as_deref().unwrap_or("").to_string();
    let l2_content = vec![Span::styled(
        ctx,
        Style::default().fg(Color::DarkGray).bg(bg),
    )];

    // Line 3: namespace
    let ns = entry.namespace.as_deref().unwrap_or("").to_string();
    let l3_content = vec![Span::styled(
        ns,
        Style::default().fg(Color::DarkGray).bg(bg),
    )];

    // Line 4: resource type/name
    let resource = entry.resource_identifier();
    let l4_content = vec![Span::styled(
        resource,
        Style::default().fg(COLOR_K8S_LABEL).bg(bg),
    )];

    // Line 5: port-binding summary
    let bindings = k8s_bindings_summary(&entry.forwards);
    let l5_content = vec![Span::styled(
        bindings,
        Style::default().fg(COLOR_FORWARD_LABEL).bg(bg),
    )];

    // Line 6: blank
    let l6_content: Vec<Span> = vec![];

    let lines = vec![
        entry_line(strip_color, bg, l1_content),
        entry_line(strip_color, bg, l2_content),
        entry_line(strip_color, bg, l3_content),
        entry_line(strip_color, bg, l4_content),
        entry_line(strip_color, bg, l5_content),
        entry_line(strip_color, bg, l6_content),
    ];

    let visible: Vec<Line> = lines.into_iter().take(area.height as usize).collect();
    let padded: Vec<Line> = visible.into_iter().map(|l| pad_line(l, w)).collect();
    frame.render_widget(Paragraph::new(padded), area);
}

// ── sshuttle entry row ───────────────────────────────────────────────────

/// Render a sshuttle entry row as exactly 6 lines.
fn render_sshuttle_entry_row(
    frame: &mut Frame,
    area: Rect,
    entry: &SshuttleEntry,
    state: ConnectionState,
    is_selected: bool,
) {
    let bg = if is_selected {
        COLOR_SELECTED_BG
    } else {
        Color::Reset
    };
    let strip_color = state_to_color(state);
    let state_label = format!("[{}]", state.label());
    let w = area.width;

    // Line 1
    let mut l1_content = vec![
        Span::styled(
            "[sshuttle] ",
            Style::default().fg(COLOR_SSHUTTLE_LABEL).bg(bg),
        ),
        Span::styled(
            format!("{} ", entry.name),
            Style::default()
                .fg(Color::White)
                .bg(bg)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{state_label} "),
            Style::default().fg(strip_color).bg(bg),
        ),
    ];
    l1_content.extend(auto_restart_spans(entry.auto_restart, bg));

    // Line 2: host or host:port
    let host_str = if let Some(p) = entry.port {
        format!("{}:{p}", entry.host)
    } else {
        entry.host.clone()
    };
    let l2_content = vec![Span::styled(
        host_str,
        Style::default().fg(Color::White).bg(bg),
    )];

    // Line 3: identity
    let identity = match (&entry.user, &entry.identity_file) {
        (Some(u), Some(k)) => format!("user:{u}  key:{k}"),
        (Some(u), None) => format!("user:{u}"),
        (None, Some(k)) => format!("key:{k}"),
        (None, None) => String::new(),
    };
    let l3_content = vec![Span::styled(
        identity,
        Style::default().fg(Color::DarkGray).bg(bg),
    )];

    // Line 4: subnet summary
    let subnet_summary = match entry.subnets.len() {
        0 => String::new(),
        1 => entry.subnets[0].clone(),
        n => format!("{n} subnets"),
    };
    let l4_content = vec![Span::styled(
        subnet_summary,
        Style::default().fg(COLOR_SSHUTTLE_LABEL).bg(bg),
    )];

    // Lines 5–6: blank
    let blank: Vec<Span> = vec![];

    let lines = vec![
        entry_line(strip_color, bg, l1_content),
        entry_line(strip_color, bg, l2_content),
        entry_line(strip_color, bg, l3_content),
        entry_line(strip_color, bg, l4_content),
        entry_line(strip_color, bg, blank.clone()),
        entry_line(strip_color, bg, blank),
    ];

    let visible: Vec<Line> = lines.into_iter().take(area.height as usize).collect();
    let padded: Vec<Line> = visible.into_iter().map(|l| pad_line(l, w)).collect();
    frame.render_widget(Paragraph::new(padded), area);
}

/// Return the line as-is — ratatui clips to the widget area automatically.
fn pad_line(line: Line<'_>, _width: u16) -> Line<'_> {
    line
}

// ── Details panel ────────────────────────────────────────────────────────

/// Render the details panel: splash by default, form/type-select when active.
fn render_details_panel(frame: &mut Frame, area: Rect, app: &App) {
    match &app.mode {
        AppMode::Normal => render_details_splash(frame, area, app),
        AppMode::TypeSelect(sel) => render_type_select_overlay(frame, area, sel),
        AppMode::Form(form_state) => render_form_overlay(frame, area, form_state),
    }
}

/// Render the enhanced STunT splash in the details panel.
fn render_details_splash(frame: &mut Frame, area: Rect, app: &App) {
    let version = env!("CARGO_PKG_VERSION");

    let total = app.entries.len();
    let connected = app.count_in_state(ConnectionState::Connected);
    let failed = app.count_in_state(ConnectionState::Failed);
    let suspended = app.count_in_state(ConnectionState::Suspended);

    let mut summary = format!("{total} entries · {connected} connected · {failed} failed");
    if suspended > 0 {
        summary.push_str(&format!(" · {suspended} suspended"));
    }

    let logo_style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);

    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled(
            r"  ____  _____            _____  ",
            logo_style,
        )),
        Line::from(Span::styled(
            r" / ___||_   _|_   _ _ __|_   _| ",
            logo_style,
        )),
        Line::from(Span::styled(r" \___ \  | | | | | | '_ \| |   ", logo_style)),
        Line::from(Span::styled(r"  ___) | | | | |_| | | | | |   ", logo_style)),
        Line::from(Span::styled(r" |____/  |_|  \__,_|_| |_|_|   ", logo_style)),
        Line::from(Span::styled(
            format!("v{version}"),
            Style::default().fg(Color::White),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Stupid Tunnel Tricks",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(summary, Style::default().fg(Color::DarkGray))),
    ];

    // In Normal mode with no entries, add the hint
    if app.entries.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Press 'n' to create a tunnel entry.",
            Style::default().fg(Color::DarkGray),
        )));
    }

    let content_height = lines.len() as u16;
    let vertical_pad = area.height.saturating_sub(content_height) / 2;

    let splash_area = Layout::vertical([
        Constraint::Length(vertical_pad),
        Constraint::Length(content_height),
        Constraint::Min(0),
    ])
    .split(area)[1];

    let splash = Paragraph::new(lines).alignment(Alignment::Center);
    frame.render_widget(splash, splash_area);
}

// ── Type selection overlay ───────────────────────────────────────────────

/// Render the entry type selection content centered within the details panel.
fn render_type_select_overlay(frame: &mut Frame, area: Rect, sel: &EntryTypeSelection) {
    let options = ["SSH Server", "Kubernetes Workload", "sshuttle VPN"];

    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled(
            "New Entry — Select Type",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];
    for (i, opt) in options.iter().enumerate() {
        let is_selected = i == sel.selected;
        let style = if is_selected {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        let prefix = if is_selected { "> " } else { "  " };
        lines.push(Line::from(Span::styled(format!("{prefix}{opt}"), style)));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Tab/↑↓: select  Enter: confirm  Esc: cancel",
        Style::default().fg(Color::DarkGray),
    )));

    let content_height = lines.len() as u16;
    let vertical_pad = area.height.saturating_sub(content_height) / 2;
    let content_area = Layout::vertical([
        Constraint::Length(vertical_pad),
        Constraint::Length(content_height),
        Constraint::Min(0),
    ])
    .split(area)[1];

    frame.render_widget(
        Paragraph::new(lines).alignment(Alignment::Center),
        content_area,
    );
}

// ── Form overlay ─────────────────────────────────────────────────────────

/// Render the form content centered within the details panel — no border or background.
fn render_form_overlay(frame: &mut Frame, area: Rect, form: &FormState) {
    let is_edit = form.editing_id.is_some();
    let title = match (&form.entry_type, is_edit) {
        (FormEntryType::Ssh, false) => "New SSH Entry",
        (FormEntryType::Ssh, true) => "Edit SSH Entry",
        (FormEntryType::K8s, false) => "New K8s Entry",
        (FormEntryType::K8s, true) => "Edit K8s Entry",
        (FormEntryType::Sshuttle, false) => "New sshuttle Entry",
        (FormEntryType::Sshuttle, true) => "Edit sshuttle Entry",
    };

    let is_editing_forward = matches!(form.focus, FormFocus::ForwardEdit { .. });

    let mut lines: Vec<Line> = Vec::new();

    // Title line
    lines.push(Line::from(Span::styled(
        title,
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));

    let server_fields_focused = matches!(form.focus, FormFocus::ServerFields);
    for (i, field) in form.fields.iter().enumerate() {
        let is_focused = server_fields_focused && i == form.focused_field;
        let label_style = if is_focused {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let is_toggle = field.label == "Auto Restart";
        let is_cycle = field.label == "Resource Type";

        if is_toggle {
            let yes_style = if field.value == "yes" {
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let no_style = if field.value != "yes" {
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let hint = if is_focused {
                "  (space to toggle)"
            } else {
                ""
            };
            lines.push(Line::from(vec![
                Span::styled(format!("{:>14}: ", field.label), label_style),
                Span::styled("yes", yes_style),
                Span::raw(" / "),
                Span::styled("no", no_style),
                Span::styled(hint, Style::default().fg(Color::DarkGray)),
            ]));
        } else if is_cycle {
            let hint = if is_focused { "  (type to change)" } else { "" };
            lines.push(Line::from(vec![
                Span::styled(format!("{:>14}: ", field.label), label_style),
                Span::styled(
                    &field.value,
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(hint, Style::default().fg(Color::DarkGray)),
            ]));
        } else {
            let value_style = if is_focused {
                Style::default().fg(Color::White).bg(Color::Rgb(40, 40, 60))
            } else {
                Style::default().fg(Color::White)
            };
            let cursor = if is_focused { "_" } else { "" };
            lines.push(Line::from(vec![
                Span::styled(format!("{:>14}: ", field.label), label_style),
                Span::styled(format!("{}{}", field.value, cursor), value_style),
            ]));
        }
    }

    if !matches!(form.entry_type, FormEntryType::Sshuttle) {
        lines.push(Line::from(""));

        let hints = match form.focus {
            FormFocus::ForwardList => "(Enter edit  Ctrl+A add  Ctrl+D del)",
            FormFocus::ForwardEdit { .. } => "(Enter save  Esc cancel)",
            FormFocus::ServerFields => "(Ctrl+A add  Tab to list)",
        };
        let fwd_section_label = match form.entry_type {
            FormEntryType::Ssh => "  Forwards ",
            FormEntryType::K8s => "  Port Bindings ",
            FormEntryType::Sshuttle => unreachable!(),
        };
        lines.push(Line::from(vec![
            Span::styled(
                fwd_section_label,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(hints, Style::default().fg(Color::DarkGray)),
        ]));

        let in_forward_list = matches!(form.focus, FormFocus::ForwardList);
        match form.entry_type {
            FormEntryType::Ssh => {
                for (i, fwd) in form.forwards.iter().enumerate() {
                    let type_label = fwd.type_label();
                    let addr = fwd.display_address();
                    let is_sel = in_forward_list && i == form.selected_forward;
                    let (type_style, addr_style) = forward_styles(is_sel);
                    let prefix = if is_sel { "  > " } else { "    " };
                    lines.push(Line::from(vec![
                        Span::styled(format!("{prefix}[{type_label}] "), type_style),
                        Span::styled(addr, addr_style),
                    ]));
                }
                if form.forwards.is_empty() && !is_editing_forward {
                    lines.push(Line::from(vec![Span::styled(
                        "    (none — Ctrl+A to add)",
                        Style::default().fg(Color::DarkGray),
                    )]));
                }
            }
            FormEntryType::K8s => {
                for (i, fwd) in form.k8s_forwards.iter().enumerate() {
                    let addr = fwd.display_address();
                    let is_sel = in_forward_list && i == form.selected_forward;
                    let (_type_style, addr_style) = forward_styles(is_sel);
                    let prefix = if is_sel { "  > " } else { "    " };
                    lines.push(Line::from(vec![
                        Span::styled(
                            format!("{prefix}[K] "),
                            Style::default().fg(COLOR_K8S_LABEL),
                        ),
                        Span::styled(addr, addr_style),
                    ]));
                }
                if form.k8s_forwards.is_empty() && !is_editing_forward {
                    lines.push(Line::from(vec![Span::styled(
                        "    (none — Ctrl+A to add)",
                        Style::default().fg(Color::DarkGray),
                    )]));
                }
            }
            FormEntryType::Sshuttle => unreachable!(),
        }
    }

    if is_editing_forward {
        let editing_label = match form.focus {
            FormFocus::ForwardEdit {
                editing_index: Some(_),
            } => "Editing binding",
            _ => "New binding",
        };

        let type_desc = match form.entry_type {
            FormEntryType::Ssh => {
                let type_name = match form.forward_type {
                    0 => "Local (-L)",
                    1 => "Remote (-R)",
                    2 => "Dynamic (-D)",
                    _ => "Unknown",
                };
                format!("{editing_label} — Type: {type_name}  (Ctrl+T to cycle)")
            }
            FormEntryType::K8s => format!("{editing_label} — kubectl port-forward"),
            FormEntryType::Sshuttle => unreachable!("sshuttle has no forward sub-form"),
        };

        lines.push(Line::from(vec![Span::styled(
            format!("    {type_desc}"),
            Style::default().fg(COLOR_TRANSIENT),
        )]));

        let num_fields = match form.entry_type {
            FormEntryType::Ssh => {
                if form.forward_type == 2 {
                    1
                } else {
                    3
                }
            }
            FormEntryType::K8s => 2,
            FormEntryType::Sshuttle => unreachable!("sshuttle has no forward sub-form"),
        };

        for (fi, field) in form.forward_fields.iter().enumerate().take(num_fields) {
            let is_focused = fi == form.forward_field;
            let label_style = if is_focused {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let value_style = if is_focused {
                Style::default().fg(Color::White).bg(Color::Rgb(40, 40, 60))
            } else {
                Style::default().fg(Color::White)
            };
            let cursor = if is_focused { "_" } else { "" };
            lines.push(Line::from(vec![
                Span::styled(format!("    {:>14}: ", field.label), label_style),
                Span::styled(format!("{}{}", field.value, cursor), value_style),
            ]));
        }
    }

    lines.push(Line::from(""));
    let bottom_hints = match form.focus {
        FormFocus::ForwardEdit { .. } => "  Enter: save  Esc: cancel  Tab/Shift+Tab: fields",
        FormFocus::ForwardList => "  Enter: edit  Esc: cancel form  Tab/Shift+Tab: navigate",
        FormFocus::ServerFields => "  Enter: save entry  Esc: cancel  Tab/Shift+Tab: fields",
    };
    lines.push(Line::from(vec![Span::styled(
        bottom_hints,
        Style::default().fg(Color::DarkGray),
    )]));

    // Center the form content horizontally within the details panel
    let content_width = 60u16.min(area.width);
    let h_pad = area.width.saturating_sub(content_width) / 2;
    let h_area = Layout::horizontal([
        Constraint::Length(h_pad),
        Constraint::Length(content_width),
        Constraint::Min(0),
    ])
    .split(area)[1];

    // Center the form content vertically within that column
    let content_height = (lines.len() as u16).min(h_area.height);
    let vertical_pad = h_area.height.saturating_sub(content_height) / 2;
    let content_area = Layout::vertical([
        Constraint::Length(vertical_pad),
        Constraint::Length(content_height),
        Constraint::Min(0),
    ])
    .split(h_area)[1];

    let form_content = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(form_content, content_area);
}

// ── Utilities ─────────────────────────────────────────────────────────────

/// Map a `ConnectionState` to a display color.
fn state_to_color(state: ConnectionState) -> Color {
    match state {
        ConnectionState::Connected => COLOR_CONNECTED,
        ConnectionState::Failed => COLOR_FAILED,
        ConnectionState::Connecting | ConnectionState::Reconnecting => COLOR_TRANSIENT,
        ConnectionState::Disconnected => COLOR_DISCONNECTED,
        ConnectionState::Suspended => COLOR_SUSPENDED,
    }
}

/// Returns (type_style, addr_style) for a forward list row, selected or not.
fn forward_styles(is_selected: bool) -> (Style, Style) {
    if is_selected {
        (
            Style::default()
                .fg(COLOR_FORWARD_LABEL)
                .bg(Color::Rgb(40, 40, 60))
                .add_modifier(Modifier::BOLD),
            Style::default().fg(Color::White).bg(Color::Rgb(40, 40, 60)),
        )
    } else {
        (
            Style::default().fg(COLOR_FORWARD_LABEL),
            Style::default().fg(Color::White),
        )
    }
}
