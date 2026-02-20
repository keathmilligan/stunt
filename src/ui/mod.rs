//! UI components and rendering.
//!
//! Implements the three-region layout (title bar, list viewport, status bar),
//! multi-line server entry rows with color-coded state, variable-height
//! scrolling, and modal input forms for creating/editing entries.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::app::{App, AppMode, FormFocus, FormState};
use crate::tunnel::ConnectionState;

// ── Color palette ──────────────────────────────────────────────────────

/// Color for Connected state.
const COLOR_CONNECTED: Color = Color::Green;
/// Color for Failed state.
const COLOR_FAILED: Color = Color::Red;
/// Color for Connecting / Reconnecting state.
const COLOR_TRANSIENT: Color = Color::Yellow;
/// Color for Disconnected state.
const COLOR_DISCONNECTED: Color = Color::DarkGray;
/// Color for Suspended state.
const COLOR_SUSPENDED: Color = Color::Magenta;
/// Color for forward type labels (L, R, D).
const COLOR_FORWARD_LABEL: Color = Color::Cyan;
/// Background color for the title bar.
const COLOR_TITLE_BG: Color = Color::Blue;
/// Background color for the status bar.
const COLOR_STATUS_BG: Color = Color::DarkGray;
/// Background color for the selected row.
const COLOR_SELECTED_BG: Color = Color::Rgb(30, 40, 60);

// ── Public entry point ─────────────────────────────────────────────────

/// Render the entire application UI.
///
/// This is the top-level `ui()` function called from the main loop.
/// It dispatches to either the normal list view or the form overlay
/// based on the current app mode.
pub fn draw(frame: &mut Frame, app: &App) {
    // Three-region layout: title bar (1), list viewport (fill), status bar (1)
    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .split(frame.area());

    render_title_bar(frame, chunks[0]);
    render_list_viewport(frame, chunks[1], app);
    render_status_bar(frame, chunks[2], app);

    // If in form mode, draw the modal overlay on top
    if let AppMode::Form(ref form_state) = app.mode {
        render_form_overlay(frame, frame.area(), form_state);
    }
}

// ── Title bar ──────────────────────────────────────────────────────────

/// Render the title bar with app name (left) and key hints (right).
fn render_title_bar(frame: &mut Frame, area: Rect) {
    let style = Style::default()
        .fg(Color::White)
        .bg(COLOR_TITLE_BG)
        .add_modifier(Modifier::BOLD);

    let title = "tunnel-mgr";
    let hints = "[n]ew  [e]dit  [d]elete  [Enter] connect  [q]uit";

    // Calculate spacing between title and hints
    let available = area.width as usize;
    let title_len = title.len();
    let hints_len = hints.len();

    let line = if available >= title_len + hints_len + 2 {
        let padding = available - title_len - hints_len;
        let pad_str: String = " ".repeat(padding);
        Line::from(vec![
            Span::styled(title, style),
            Span::styled(pad_str, style),
            Span::styled(hints, style),
        ])
    } else {
        // Not enough room for hints, just show title
        let padding = available.saturating_sub(title_len);
        let pad_str: String = " ".repeat(padding);
        Line::from(vec![
            Span::styled(title, style),
            Span::styled(pad_str, style),
        ])
    };

    frame.render_widget(Paragraph::new(line), area);
}

// ── Status bar ─────────────────────────────────────────────────────────

/// Render the status bar with connection summary and transient messages.
fn render_status_bar(frame: &mut Frame, area: Rect, app: &App) {
    let style = Style::default().fg(Color::White).bg(COLOR_STATUS_BG);

    let total = app.entries.len();
    let connected = app.count_in_state(ConnectionState::Connected);
    let failed = app.count_in_state(ConnectionState::Failed);
    let suspended = app.count_in_state(ConnectionState::Suspended);

    let mut summary = format!(" {total} entries  {connected} connected  {failed} failed");
    if suspended > 0 {
        summary.push_str(&format!("  {suspended} suspended"));
    }

    let line = if let Some(msg) = app.active_status_message() {
        let available = area.width as usize;
        let msg_len = msg.len();
        let summary_len = summary.len();
        if available >= summary_len + msg_len + 4 {
            let padding = available - summary_len - msg_len - 2;
            let pad_str: String = " ".repeat(padding);
            Line::from(vec![
                Span::styled(summary, style),
                Span::styled(pad_str, style),
                Span::styled(
                    format!("{msg} "),
                    Style::default().fg(COLOR_TRANSIENT).bg(COLOR_STATUS_BG),
                ),
            ])
        } else {
            Line::from(vec![Span::styled(summary, style)])
        }
    } else {
        let padding = (area.width as usize).saturating_sub(summary.len());
        let pad_str: String = " ".repeat(padding);
        Line::from(vec![
            Span::styled(summary, style),
            Span::styled(pad_str, style),
        ])
    };

    frame.render_widget(Paragraph::new(line), area);
}

// ── List viewport ──────────────────────────────────────────────────────

/// Compute the height (in lines) of a server entry row.
fn row_height(entry: &crate::config::ServerEntry) -> u16 {
    // Header line + one line per forward (minimum 1 total)
    1 + entry.forwards.len().max(0) as u16
}

/// Render the scrollable list of server entries.
fn render_list_viewport(frame: &mut Frame, area: Rect, app: &App) {
    if app.entries.is_empty() {
        let empty_msg = Paragraph::new(Line::from(vec![Span::styled(
            "  No tunnel entries. Press 'n' to create one.",
            Style::default().fg(Color::DarkGray),
        )]));
        frame.render_widget(empty_msg, area);
        return;
    }

    let viewport_height = area.height as usize;

    // Compute cumulative heights and adjust scroll offset
    let heights: Vec<usize> = app.entries.iter().map(|e| row_height(e) as usize).collect();

    // Adjust scroll offset to keep selected row fully visible
    let scroll_offset =
        compute_scroll_offset(app.selected, app.scroll_offset, &heights, viewport_height);

    // Render visible rows
    let mut y_offset: u16 = 0;
    let mut cumulative = 0usize;

    for (i, entry) in app.entries.iter().enumerate() {
        let h = heights[i];

        if cumulative + h <= scroll_offset {
            cumulative += h;
            continue;
        }

        if y_offset >= area.height {
            break;
        }

        let is_selected = i == app.selected;
        let state = app.state_of(&entry.id);

        let row_area = Rect {
            x: area.x,
            y: area.y + y_offset,
            width: area.width,
            height: (h as u16).min(area.height - y_offset),
        };

        render_entry_row(frame, row_area, entry, state, is_selected);

        y_offset += h as u16;
        cumulative += h;
    }
}

/// Compute the adjusted scroll offset to keep the selected row fully visible.
fn compute_scroll_offset(
    selected: usize,
    current_offset: usize,
    heights: &[usize],
    viewport_height: usize,
) -> usize {
    if heights.is_empty() || viewport_height == 0 {
        return 0;
    }

    // Compute the top (cumulative lines before selected) and bottom of selected row
    let top_of_selected: usize = heights[..selected].iter().sum();
    let bottom_of_selected = top_of_selected + heights[selected];

    let mut offset = current_offset;

    // If selected row starts above the viewport, scroll up
    if top_of_selected < offset {
        offset = top_of_selected;
    }

    // If selected row ends below the viewport, scroll down
    if bottom_of_selected > offset + viewport_height {
        offset = bottom_of_selected.saturating_sub(viewport_height);
    }

    offset
}

/// Render a single server entry row.
fn render_entry_row(
    frame: &mut Frame,
    area: Rect,
    entry: &crate::config::ServerEntry,
    state: ConnectionState,
    is_selected: bool,
) {
    let bg = if is_selected {
        COLOR_SELECTED_BG
    } else {
        Color::Reset
    };

    let state_color = match state {
        ConnectionState::Connected => COLOR_CONNECTED,
        ConnectionState::Failed => COLOR_FAILED,
        ConnectionState::Connecting | ConnectionState::Reconnecting => COLOR_TRANSIENT,
        ConnectionState::Disconnected => COLOR_DISCONNECTED,
        ConnectionState::Suspended => COLOR_SUSPENDED,
    };

    let state_indicator = format!("[{}]", state.label());

    // Build header line: name  [state]  host:port  user  identity
    let mut header_spans = vec![
        Span::styled(
            format!("  {} ", entry.name),
            Style::default()
                .fg(Color::White)
                .bg(bg)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{state_indicator} "),
            Style::default().fg(state_color).bg(bg),
        ),
        Span::styled(
            format!("{}:{}", entry.host, entry.port),
            Style::default().fg(Color::White).bg(bg),
        ),
    ];

    if let Some(ref user) = entry.user {
        header_spans.push(Span::styled(
            format!("  user:{user}"),
            Style::default().fg(Color::DarkGray).bg(bg),
        ));
    }

    if let Some(ref id_file) = entry.identity_file {
        header_spans.push(Span::styled(
            format!("  key:{id_file}"),
            Style::default().fg(Color::DarkGray).bg(bg),
        ));
    }

    // Pad the rest of the line with background color
    header_spans.push(Span::styled(
        " ".repeat(area.width as usize),
        Style::default().bg(bg),
    ));

    let mut lines: Vec<Line> = vec![Line::from(header_spans)];

    // Forward lines
    for fwd in &entry.forwards {
        let type_label = fwd.type_label();
        let addr = fwd.display_address();

        let fwd_line = Line::from(vec![
            Span::styled(
                format!("    [{type_label}] "),
                Style::default().fg(COLOR_FORWARD_LABEL).bg(bg),
            ),
            Span::styled(addr, Style::default().fg(Color::White).bg(bg)),
            Span::styled(" ".repeat(area.width as usize), Style::default().bg(bg)),
        ]);
        lines.push(fwd_line);
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, area);
}

// ── Form overlay ───────────────────────────────────────────────────────

/// Render the modal form overlay for creating/editing a server entry.
fn render_form_overlay(frame: &mut Frame, area: Rect, form: &FormState) {
    let is_edit = form.editing_id.is_some();
    let title = if is_edit {
        " Edit Server Entry "
    } else {
        " New Server Entry "
    };

    let is_editing_forward = matches!(form.focus, FormFocus::ForwardEdit { .. });

    // Compute form dimensions
    let form_width = 60u16.min(area.width.saturating_sub(4));
    let base_height = form.fields.len() as u16 + 4; // fields + title + borders + forward header
    let forward_lines = form.forwards.len() as u16;
    let editing_fwd_lines = if is_editing_forward { 5 } else { 0 };
    let form_height =
        (base_height + forward_lines + editing_fwd_lines + 3).min(area.height.saturating_sub(2));

    let form_x = area.x + (area.width.saturating_sub(form_width)) / 2;
    let form_y = area.y + (area.height.saturating_sub(form_height)) / 2;

    let form_area = Rect {
        x: form_x,
        y: form_y,
        width: form_width,
        height: form_height,
    };

    // Clear the area behind the form
    frame.render_widget(Clear, form_area);

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));

    let inner = block.inner(form_area);
    frame.render_widget(block, form_area);

    let mut lines: Vec<Line> = Vec::new();

    // Render server fields
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

        if is_toggle {
            // Render as a toggle: [yes] / [no] with highlight
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

    // Blank line before forwards section
    lines.push(Line::from(""));

    // Forwards header with context-sensitive hints
    let hints = match form.focus {
        FormFocus::ForwardList => "(Enter edit  Ctrl+A add  Ctrl+D del)",
        FormFocus::ForwardEdit { .. } => "(Enter save  Esc cancel  Ctrl+T type)",
        FormFocus::ServerFields => "(Ctrl+A add  Tab to list)",
    };
    lines.push(Line::from(vec![
        Span::styled(
            "  Forwards ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(hints, Style::default().fg(Color::DarkGray)),
    ]));

    // Existing forwards (with selection highlight in ForwardList mode)
    let in_forward_list = matches!(form.focus, FormFocus::ForwardList);
    for (i, fwd) in form.forwards.iter().enumerate() {
        let type_label = fwd.type_label();
        let addr = fwd.display_address();
        let is_selected = in_forward_list && i == form.selected_forward;

        let (type_style, addr_style) = if is_selected {
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
        };

        let prefix = if is_selected { "  > " } else { "    " };
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

    // Forward sub-form (if editing)
    if is_editing_forward {
        let editing_label = match form.focus {
            FormFocus::ForwardEdit {
                editing_index: Some(_),
            } => "Editing forward",
            _ => "New forward",
        };
        let type_name = match form.forward_type {
            0 => "Local (-L)",
            1 => "Remote (-R)",
            2 => "Dynamic (-D)",
            _ => "Unknown",
        };
        lines.push(Line::from(vec![Span::styled(
            format!("    {editing_label} — Type: {type_name}  (Ctrl+T to cycle)"),
            Style::default().fg(COLOR_TRANSIENT),
        )]));

        let num_fields = if form.forward_type == 2 { 1 } else { 3 };
        for (fi, field) in form.forward_fields.iter().enumerate().take(num_fields) {
            let is_focused = is_editing_forward && fi == form.forward_field;
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

    // Blank line + hints
    lines.push(Line::from(""));
    let bottom_hints = match form.focus {
        FormFocus::ForwardEdit { .. } => {
            "  Enter: save forward  Esc: cancel  Tab/Shift+Tab: fields"
        }
        FormFocus::ForwardList => {
            "  Enter: edit forward  Esc: cancel form  Tab/Shift+Tab: navigate"
        }
        FormFocus::ServerFields => "  Enter: save entry  Esc: cancel  Tab/Shift+Tab: fields",
    };
    lines.push(Line::from(vec![Span::styled(
        bottom_hints,
        Style::default().fg(Color::DarkGray),
    )]));

    let form_content = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(form_content, inner);
}
