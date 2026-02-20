//! Core application state and update loop.

use std::collections::HashMap;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use tokio::sync::mpsc;
use uuid::Uuid;

use crate::config::{self, Config, ServerEntry, TunnelForward};
use crate::config::{SessionRecord, SessionState, load_sessions, save_sessions};
use crate::tunnel::{ConnectionState, Supervisor, TunnelEvent, is_live_ssh_tunnel};

/// How long transient status messages are shown (in seconds).
const STATUS_MSG_DURATION_SECS: u64 = 3;

/// The current mode of the application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppMode {
    /// Normal list view.
    Normal,
    /// Editing or creating a server entry.
    Form(FormState),
}

/// Which section of the form currently has focus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormFocus {
    /// Focused on the server connection fields (name, host, port, etc.).
    ServerFields,
    /// Focused on the forwards list, selecting an existing forward.
    ForwardList,
    /// Actively editing a forward's fields (new or existing).
    ForwardEdit {
        /// Index of the forward being edited, or `None` for a new forward.
        editing_index: Option<usize>,
    },
}

/// State for the server entry form (new or edit).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormState {
    /// Which field is currently focused in the server fields section.
    pub focused_field: usize,
    /// Whether we are editing an existing entry (Some(id)) or creating new (None).
    pub editing_id: Option<Uuid>,
    /// Field values: name, host, port, user, identity_file.
    pub fields: Vec<FormField>,
    /// Forwards being edited.
    pub forwards: Vec<TunnelForward>,
    /// Which section of the form currently has focus.
    pub focus: FormFocus,
    /// Currently selected forward index in the list.
    pub selected_forward: usize,
    /// Current forward field being edited (within the forward sub-form).
    pub forward_field: usize,
    /// Forward type selector index (0=Local, 1=Remote, 2=Dynamic).
    pub forward_type: usize,
    /// Forward field values: bind_port, remote_host, remote_port.
    pub forward_fields: Vec<FormField>,
}

/// A single form field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormField {
    /// Field label.
    pub label: String,
    /// Current value.
    pub value: String,
}

/// Messages that drive state transitions in the app.
#[derive(Debug)]
pub enum Message {
    /// Move selection up.
    NavigateUp,
    /// Move selection down.
    NavigateDown,
    /// Start creating a new entry.
    NewEntry,
    /// Start editing the selected entry.
    EditEntry,
    /// Delete the selected entry.
    DeleteEntry,
    /// Toggle connect/disconnect for the selected entry.
    ToggleConnect,
    /// Quit the application.
    Quit,
    /// A tunnel lifecycle event.
    TunnelEvent(TunnelEvent),
    /// Form: move to next field.
    FormNextField,
    /// Form: move to previous field.
    FormPrevField,
    /// Form: type a character into the current field.
    FormInput(char),
    /// Form: delete character (backspace).
    FormBackspace,
    /// Form: submit the form.
    FormSubmit,
    /// Form: cancel and return to normal mode.
    FormCancel,
    /// Form: add a new forward.
    FormAddForward,
    /// Form: delete last forward.
    FormDeleteForward,
    /// Form: cycle forward type.
    FormCycleForwardType,
}

/// Core application state.
pub struct App {
    /// All configured server entries.
    pub entries: Vec<ServerEntry>,
    /// Currently selected entry index.
    pub selected: usize,
    /// Scroll offset (in rows, not lines).
    pub scroll_offset: usize,
    /// Connection state per entry (by entry id).
    pub connection_states: HashMap<Uuid, ConnectionState>,
    /// Active supervisors per entry (by entry id).
    supervisors: HashMap<Uuid, Supervisor>,
    /// Runtime session state (PID tracking, persisted to sessions.json).
    sessions: SessionState,
    /// Channel sender for tunnel events.
    tunnel_tx: mpsc::UnboundedSender<TunnelEvent>,
    /// Transient status message and when it was set.
    pub status_message: Option<(String, Instant)>,
    /// Whether the app is still running.
    pub running: bool,
    /// Current app mode.
    pub mode: AppMode,
}

impl App {
    /// Create a new App from a loaded config and a tunnel event sender.
    pub fn new(config: Config, tunnel_tx: mpsc::UnboundedSender<TunnelEvent>) -> Self {
        let mut connection_states = HashMap::new();
        for entry in &config.server {
            connection_states.insert(entry.id, ConnectionState::Disconnected);
        }

        let sessions = load_sessions();

        let mut app = App {
            entries: config.server,
            selected: 0,
            scroll_offset: 0,
            connection_states,
            supervisors: HashMap::new(),
            sessions,
            tunnel_tx,
            status_message: None,
            running: true,
            mode: AppMode::Normal,
        };

        app.reconcile_sessions();
        app
    }

    /// Get the active status message if it hasn't expired.
    pub fn active_status_message(&self) -> Option<&str> {
        if let Some((ref msg, when)) = self.status_message
            && when.elapsed().as_secs() < STATUS_MSG_DURATION_SECS
        {
            return Some(msg.as_str());
        }
        None
    }

    /// Set a transient status message.
    fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = Some((msg.into(), Instant::now()));
    }

    /// Get the connection state for an entry.
    pub fn state_of(&self, id: &Uuid) -> ConnectionState {
        self.connection_states
            .get(id)
            .copied()
            .unwrap_or(ConnectionState::Disconnected)
    }

    /// Count entries in a given connection state.
    pub fn count_in_state(&self, state: ConnectionState) -> usize {
        self.connection_states
            .values()
            .filter(|s| **s == state)
            .count()
    }

    /// Process a message and update state.
    pub fn update(&mut self, msg: Message) {
        match self.mode {
            AppMode::Normal => self.update_normal(msg),
            AppMode::Form(_) => self.update_form(msg),
        }
    }

    /// Handle messages in normal (list) mode.
    fn update_normal(&mut self, msg: Message) {
        match msg {
            Message::NavigateUp => {
                if self.selected > 0 {
                    self.selected -= 1;
                }
            }
            Message::NavigateDown => {
                if !self.entries.is_empty() && self.selected < self.entries.len() - 1 {
                    self.selected += 1;
                }
            }
            Message::NewEntry => {
                self.mode = AppMode::Form(FormState {
                    focused_field: 0,
                    editing_id: None,
                    fields: vec![
                        FormField {
                            label: "Name".to_string(),
                            value: String::new(),
                        },
                        FormField {
                            label: "Host".to_string(),
                            value: String::new(),
                        },
                        FormField {
                            label: "Port".to_string(),
                            value: "22".to_string(),
                        },
                        FormField {
                            label: "User".to_string(),
                            value: String::new(),
                        },
                        FormField {
                            label: "Identity File".to_string(),
                            value: String::new(),
                        },
                        FormField {
                            label: "Auto Restart".to_string(),
                            value: "no".to_string(),
                        },
                    ],
                    forwards: vec![],
                    focus: FormFocus::ServerFields,
                    selected_forward: 0,
                    forward_field: 0,
                    forward_type: 0,
                    forward_fields: Self::default_forward_fields(),
                });
            }
            Message::EditEntry => {
                if let Some(entry) = self.entries.get(self.selected) {
                    self.mode = AppMode::Form(FormState {
                        focused_field: 0,
                        editing_id: Some(entry.id),
                        fields: vec![
                            FormField {
                                label: "Name".to_string(),
                                value: entry.name.clone(),
                            },
                            FormField {
                                label: "Host".to_string(),
                                value: entry.host.clone(),
                            },
                            FormField {
                                label: "Port".to_string(),
                                value: entry.port.to_string(),
                            },
                            FormField {
                                label: "User".to_string(),
                                value: entry.user.clone().unwrap_or_default(),
                            },
                            FormField {
                                label: "Identity File".to_string(),
                                value: entry.identity_file.clone().unwrap_or_default(),
                            },
                            FormField {
                                label: "Auto Restart".to_string(),
                                value: if entry.auto_restart {
                                    "yes".to_string()
                                } else {
                                    "no".to_string()
                                },
                            },
                        ],
                        forwards: entry.forwards.clone(),
                        focus: FormFocus::ServerFields,
                        selected_forward: 0,
                        forward_field: 0,
                        forward_type: 0,
                        forward_fields: Self::default_forward_fields(),
                    });
                }
            }
            Message::DeleteEntry => {
                if let Some(entry) = self.entries.get(self.selected) {
                    let id = entry.id;
                    // Disconnect if active
                    if self.state_of(&id).is_active() {
                        self.disconnect(id);
                    }
                    self.connection_states.remove(&id);
                    self.sessions.remove(&id);
                    self.entries.remove(self.selected);
                    // Adjust selection
                    if self.selected >= self.entries.len() && self.selected > 0 {
                        self.selected -= 1;
                    }
                    self.save_config();
                    self.persist_sessions();
                    self.set_status("Entry deleted");
                }
            }
            Message::ToggleConnect => {
                if let Some(entry) = self.entries.get(self.selected) {
                    let id = entry.id;
                    let name = entry.name.clone();
                    let state = self.state_of(&id);
                    if state.is_active() {
                        self.disconnect(id);
                        self.set_status(format!("Disconnecting: {name}"));
                    } else if state == ConnectionState::Suspended {
                        // Clear suspended state and reconnect
                        self.sessions.remove(&id);
                        self.persist_sessions();
                        self.connect(self.selected);
                        self.set_status(format!("Connecting: {name}"));
                    } else {
                        self.connect(self.selected);
                        self.set_status(format!("Connecting: {name}"));
                    }
                }
            }
            Message::Quit => {
                self.running = false;
            }
            Message::TunnelEvent(event) => {
                self.handle_tunnel_event(event);
            }
            _ => {}
        }
    }

    /// Handle messages in form mode.
    fn update_form(&mut self, msg: Message) {
        let form = match &mut self.mode {
            AppMode::Form(f) => f,
            _ => return,
        };

        match msg {
            Message::FormNextField => {
                match form.focus {
                    FormFocus::ServerFields => {
                        if form.focused_field < form.fields.len() - 1 {
                            form.focused_field += 1;
                        } else if !form.forwards.is_empty() {
                            // Tab past last server field → enter forward list
                            form.focus = FormFocus::ForwardList;
                            form.selected_forward = 0;
                        }
                    }
                    FormFocus::ForwardList => {
                        if form.selected_forward < form.forwards.len().saturating_sub(1) {
                            form.selected_forward += 1;
                        }
                    }
                    FormFocus::ForwardEdit { .. } => {
                        let max = if form.forward_type == 2 { 0 } else { 2 };
                        if form.forward_field < max {
                            form.forward_field += 1;
                        }
                    }
                }
            }
            Message::FormPrevField => {
                match form.focus {
                    FormFocus::ServerFields => {
                        if form.focused_field > 0 {
                            form.focused_field -= 1;
                        }
                    }
                    FormFocus::ForwardList => {
                        if form.selected_forward > 0 {
                            form.selected_forward -= 1;
                        } else {
                            // Shift-Tab from top of forward list → back to server fields
                            form.focus = FormFocus::ServerFields;
                            form.focused_field = form.fields.len() - 1;
                        }
                    }
                    FormFocus::ForwardEdit { .. } => {
                        if form.forward_field > 0 {
                            form.forward_field -= 1;
                        }
                    }
                }
            }
            Message::FormInput(ch) => {
                match form.focus {
                    FormFocus::ServerFields => {
                        if let Some(field) = form.fields.get_mut(form.focused_field) {
                            if field.label == "Auto Restart" {
                                // Toggle on space; ignore other characters
                                if ch == ' ' {
                                    field.value = if field.value == "yes" {
                                        "no".to_string()
                                    } else {
                                        "yes".to_string()
                                    };
                                }
                            } else {
                                field.value.push(ch);
                            }
                        }
                    }
                    FormFocus::ForwardList => {
                        // No text input while browsing the forward list
                    }
                    FormFocus::ForwardEdit { .. } => {
                        if let Some(field) = form.forward_fields.get_mut(form.forward_field) {
                            field.value.push(ch);
                        }
                    }
                }
            }
            Message::FormBackspace => {
                match form.focus {
                    FormFocus::ServerFields => {
                        if let Some(field) = form.fields.get_mut(form.focused_field) {
                            if field.label == "Auto Restart" {
                                // Toggle on backspace too (toggle field, not text)
                                field.value = if field.value == "yes" {
                                    "no".to_string()
                                } else {
                                    "yes".to_string()
                                };
                            } else {
                                field.value.pop();
                            }
                        }
                    }
                    FormFocus::ForwardList => {
                        // No text input while browsing the forward list
                    }
                    FormFocus::ForwardEdit { .. } => {
                        if let Some(field) = form.forward_fields.get_mut(form.forward_field) {
                            field.value.pop();
                        }
                    }
                }
            }
            Message::FormAddForward => {
                match form.focus {
                    FormFocus::ForwardEdit { .. } => {
                        // Commit current forward, then start a new one
                        if let Some(fwd) = self.build_forward_from_form() {
                            self.commit_forward(fwd);
                            let form = match &mut self.mode {
                                AppMode::Form(f) => f,
                                _ => return,
                            };
                            form.focus = FormFocus::ForwardEdit {
                                editing_index: None,
                            };
                            form.forward_field = 0;
                            form.forward_type = 0;
                            form.forward_fields = Self::default_forward_fields();
                        }
                    }
                    _ => {
                        // Start adding a new forward
                        form.focus = FormFocus::ForwardEdit {
                            editing_index: None,
                        };
                        form.forward_field = 0;
                        form.forward_type = 0;
                        form.forward_fields = Self::default_forward_fields();
                    }
                }
            }
            Message::FormDeleteForward => {
                match form.focus {
                    FormFocus::ForwardEdit { .. } => {
                        // Cancel forward editing, return to list or server fields
                        if form.forwards.is_empty() {
                            form.focus = FormFocus::ServerFields;
                        } else {
                            form.selected_forward = form
                                .selected_forward
                                .min(form.forwards.len().saturating_sub(1));
                            form.focus = FormFocus::ForwardList;
                        }
                    }
                    FormFocus::ForwardList => {
                        // Delete the selected forward
                        if !form.forwards.is_empty() {
                            form.forwards.remove(form.selected_forward);
                            if form.forwards.is_empty() {
                                form.focus = FormFocus::ServerFields;
                                form.focused_field = form.fields.len() - 1;
                            } else {
                                form.selected_forward = form
                                    .selected_forward
                                    .min(form.forwards.len().saturating_sub(1));
                            }
                        }
                    }
                    FormFocus::ServerFields => {
                        // Nothing to delete from server fields context
                    }
                }
            }
            Message::FormCycleForwardType => {
                if matches!(form.focus, FormFocus::ForwardEdit { .. }) {
                    form.forward_type = (form.forward_type + 1) % 3;
                    form.forward_fields = Self::default_forward_fields();
                    form.forward_field = 0;
                }
            }
            Message::FormSubmit => {
                match self.mode {
                    AppMode::Form(ref form)
                        if matches!(form.focus, FormFocus::ForwardEdit { .. }) =>
                    {
                        // Commit the forward being edited
                        if let Some(fwd) = self.build_forward_from_form() {
                            self.commit_forward(fwd);
                            // Return to forward list
                            let form = match &mut self.mode {
                                AppMode::Form(f) => f,
                                _ => return,
                            };
                            form.selected_forward = form
                                .selected_forward
                                .min(form.forwards.len().saturating_sub(1));
                            form.focus = FormFocus::ForwardList;
                        }
                        return;
                    }
                    AppMode::Form(ref form) if matches!(form.focus, FormFocus::ForwardList) => {
                        // Enter on a selected forward → open it for editing
                        let idx = form.selected_forward;
                        if let Some(fwd) = form.forwards.get(idx) {
                            let (ftype, fields) = Self::forward_to_fields(fwd);
                            let form = match &mut self.mode {
                                AppMode::Form(f) => f,
                                _ => return,
                            };
                            form.focus = FormFocus::ForwardEdit {
                                editing_index: Some(idx),
                            };
                            form.forward_type = ftype;
                            form.forward_fields = fields;
                            form.forward_field = 0;
                        }
                        return;
                    }
                    _ => {}
                }
                self.submit_form();
            }
            Message::FormCancel => {
                // If editing a forward, cancel back to list; otherwise cancel form entirely
                let form = match &mut self.mode {
                    AppMode::Form(f) => f,
                    _ => return,
                };
                match form.focus {
                    FormFocus::ForwardEdit { .. } => {
                        if form.forwards.is_empty() {
                            form.focus = FormFocus::ServerFields;
                        } else {
                            form.selected_forward = form
                                .selected_forward
                                .min(form.forwards.len().saturating_sub(1));
                            form.focus = FormFocus::ForwardList;
                        }
                    }
                    _ => {
                        self.mode = AppMode::Normal;
                    }
                }
            }
            _ => {}
        }
    }

    /// Commit a built forward to the form, handling insert vs. replace.
    fn commit_forward(&mut self, fwd: TunnelForward) {
        let form = match &mut self.mode {
            AppMode::Form(f) => f,
            _ => return,
        };
        match form.focus {
            FormFocus::ForwardEdit {
                editing_index: Some(idx),
            } => {
                if idx < form.forwards.len() {
                    form.forwards[idx] = fwd;
                    form.selected_forward = idx;
                } else {
                    form.forwards.push(fwd);
                    form.selected_forward = form.forwards.len() - 1;
                }
            }
            FormFocus::ForwardEdit {
                editing_index: None,
            } => {
                form.forwards.push(fwd);
                form.selected_forward = form.forwards.len() - 1;
            }
            _ => {
                form.forwards.push(fwd);
                form.selected_forward = form.forwards.len() - 1;
            }
        }
    }

    /// Extract forward type index and form fields from an existing forward.
    fn forward_to_fields(fwd: &TunnelForward) -> (usize, Vec<FormField>) {
        match fwd {
            TunnelForward::Local {
                bind_port,
                remote_host,
                remote_port,
                ..
            } => (
                0,
                vec![
                    FormField {
                        label: "Bind Port".to_string(),
                        value: bind_port.to_string(),
                    },
                    FormField {
                        label: "Remote Host".to_string(),
                        value: remote_host.clone(),
                    },
                    FormField {
                        label: "Remote Port".to_string(),
                        value: remote_port.to_string(),
                    },
                ],
            ),
            TunnelForward::Remote {
                bind_port,
                remote_host,
                remote_port,
                ..
            } => (
                1,
                vec![
                    FormField {
                        label: "Bind Port".to_string(),
                        value: bind_port.to_string(),
                    },
                    FormField {
                        label: "Remote Host".to_string(),
                        value: remote_host.clone(),
                    },
                    FormField {
                        label: "Remote Port".to_string(),
                        value: remote_port.to_string(),
                    },
                ],
            ),
            TunnelForward::Dynamic { bind_port, .. } => (
                2,
                vec![
                    FormField {
                        label: "Bind Port".to_string(),
                        value: bind_port.to_string(),
                    },
                    FormField {
                        label: "Remote Host".to_string(),
                        value: String::new(),
                    },
                    FormField {
                        label: "Remote Port".to_string(),
                        value: String::new(),
                    },
                ],
            ),
        }
    }

    /// Build a TunnelForward from current form state.
    fn build_forward_from_form(&self) -> Option<TunnelForward> {
        let form = match &self.mode {
            AppMode::Form(f) => f,
            _ => return None,
        };

        let bind_port: u16 = form.forward_fields.first()?.value.parse().ok()?;

        match form.forward_type {
            0 => {
                // Local
                let remote_host = form.forward_fields.get(1)?.value.clone();
                let remote_port: u16 = form.forward_fields.get(2)?.value.parse().ok()?;
                if remote_host.is_empty() {
                    return None;
                }
                Some(TunnelForward::Local {
                    bind_address: "127.0.0.1".to_string(),
                    bind_port,
                    remote_host,
                    remote_port,
                })
            }
            1 => {
                // Remote
                let remote_host = form.forward_fields.get(1)?.value.clone();
                let remote_port: u16 = form.forward_fields.get(2)?.value.parse().ok()?;
                if remote_host.is_empty() {
                    return None;
                }
                Some(TunnelForward::Remote {
                    bind_address: "127.0.0.1".to_string(),
                    bind_port,
                    remote_host,
                    remote_port,
                })
            }
            2 => {
                // Dynamic
                Some(TunnelForward::Dynamic {
                    bind_address: "127.0.0.1".to_string(),
                    bind_port,
                })
            }
            _ => None,
        }
    }

    /// Default fields for the forward sub-form.
    fn default_forward_fields() -> Vec<FormField> {
        vec![
            FormField {
                label: "Bind Port".to_string(),
                value: String::new(),
            },
            FormField {
                label: "Remote Host".to_string(),
                value: String::new(),
            },
            FormField {
                label: "Remote Port".to_string(),
                value: String::new(),
            },
        ]
    }

    /// Submit the form and create/update the entry.
    fn submit_form(&mut self) {
        let form = match &self.mode {
            AppMode::Form(f) => f.clone(),
            _ => return,
        };

        let name = form.fields[0].value.trim().to_string();
        let host = form.fields[1].value.trim().to_string();
        let port: u16 = form.fields[2].value.trim().parse().unwrap_or(22);
        let user = {
            let val = form.fields[3].value.trim().to_string();
            if val.is_empty() { None } else { Some(val) }
        };
        let identity_file = {
            let val = form.fields[4].value.trim().to_string();
            if val.is_empty() { None } else { Some(val) }
        };
        let auto_restart = form.fields[5].value.trim().eq_ignore_ascii_case("yes");

        if name.is_empty() || host.is_empty() {
            self.set_status("Name and Host are required");
            return;
        }

        if let Some(id) = form.editing_id {
            // Update existing entry
            if let Some(entry) = self.entries.iter_mut().find(|e| e.id == id) {
                entry.name = name;
                entry.host = host;
                entry.port = port;
                entry.user = user;
                entry.identity_file = identity_file;
                entry.forwards = form.forwards;
                entry.auto_restart = auto_restart;
            }
            self.set_status("Entry updated");
        } else {
            // Create new entry
            let id = Uuid::new_v4();
            let entry = ServerEntry {
                id,
                name,
                host,
                port,
                user,
                identity_file,
                forwards: form.forwards,
                auto_restart,
            };
            self.entries.push(entry);
            self.connection_states
                .insert(id, ConnectionState::Disconnected);
            self.selected = self.entries.len() - 1;
            self.set_status("Entry created");
        }

        self.save_config();
        self.mode = AppMode::Normal;
    }

    /// Initiate a connection for the entry at the given index.
    fn connect(&mut self, index: usize) {
        let entry = match self.entries.get(index) {
            Some(e) => e.clone(),
            None => return,
        };
        let id = entry.id;

        self.connection_states
            .insert(id, ConnectionState::Connecting);

        let supervisor = Supervisor::spawn(entry, self.tunnel_tx.clone());

        // Record session immediately (PID will be updated via PidUpdate event)
        self.sessions.insert(
            id,
            SessionRecord {
                pid: None,
                suspended: false,
                connected_at: None,
            },
        );
        self.persist_sessions();

        self.supervisors.insert(id, supervisor);
    }

    /// Disconnect the entry with the given id.
    fn disconnect(&mut self, id: Uuid) {
        let auto_restart = self
            .entries
            .iter()
            .find(|e| e.id == id)
            .is_some_and(|e| e.auto_restart);

        if let Some(supervisor) = self.supervisors.remove(&id) {
            // Signal the supervisor to stop reconnecting before killing
            supervisor.set_suspended(true);
            supervisor.cancel_and_kill();
        }

        if auto_restart {
            // Suspended: tunnel is intentionally stopped, suppress auto-reconnect
            self.connection_states
                .insert(id, ConnectionState::Suspended);
            self.sessions.insert(
                id,
                SessionRecord {
                    pid: None,
                    suspended: true,
                    connected_at: None,
                },
            );
        } else {
            self.connection_states
                .insert(id, ConnectionState::Disconnected);
            self.sessions.remove(&id);
        }
        self.persist_sessions();
    }

    /// Handle a tunnel lifecycle event.
    fn handle_tunnel_event(&mut self, event: TunnelEvent) {
        match event {
            TunnelEvent::Connected { entry_id } => {
                self.connection_states
                    .insert(entry_id, ConnectionState::Connected);

                // Update session record with connected_at timestamp
                if let Some(record) = self.sessions.get_mut(&entry_id) {
                    record.connected_at = Some(Self::now_timestamp());
                }
                self.persist_sessions();

                if let Some(entry) = self.entries.iter().find(|e| e.id == entry_id) {
                    self.set_status(format!("Connected: {}", entry.name));
                }
            }
            TunnelEvent::Disconnected { entry_id } => {
                // The SSH process died unexpectedly (detected by PID polling).
                let auto_restart = self
                    .entries
                    .iter()
                    .find(|e| e.id == entry_id)
                    .is_some_and(|e| e.auto_restart);

                if auto_restart {
                    // Supervisor is still alive and will attempt reconnection.
                    // Show Failed briefly; supervisor will send Reconnecting next.
                    self.connection_states
                        .insert(entry_id, ConnectionState::Failed);
                    if let Some(entry) = self.entries.iter().find(|e| e.id == entry_id) {
                        self.set_status(format!("Connection lost: {}", entry.name));
                    }
                } else {
                    // Supervisor task has exited — clean up everything.
                    self.connection_states
                        .insert(entry_id, ConnectionState::Failed);
                    self.supervisors.remove(&entry_id);
                    self.sessions.remove(&entry_id);
                    self.persist_sessions();
                    if let Some(entry) = self.entries.iter().find(|e| e.id == entry_id) {
                        self.set_status(format!("Connection lost: {}", entry.name));
                    }
                }
            }
            TunnelEvent::Failed { entry_id, reason } => {
                self.connection_states
                    .insert(entry_id, ConnectionState::Failed);
                self.supervisors.remove(&entry_id);
                self.sessions.remove(&entry_id);
                self.persist_sessions();
                self.set_status(format!("Failed: {reason}"));
            }
            TunnelEvent::Reconnecting {
                entry_id,
                attempt,
                delay_secs,
            } => {
                self.connection_states
                    .insert(entry_id, ConnectionState::Reconnecting);
                if let Some(entry) = self.entries.iter().find(|e| e.id == entry_id) {
                    self.set_status(format!(
                        "Reconnecting {} (attempt {}, {}s delay)",
                        entry.name, attempt, delay_secs
                    ));
                }
            }
            TunnelEvent::PidUpdate { entry_id, pid } => {
                // Update session record with the new PID
                let record = self.sessions.entry(entry_id).or_insert(SessionRecord {
                    pid: None,
                    suspended: false,
                    connected_at: None,
                });
                record.pid = Some(pid);
                self.persist_sessions();
            }
        }
    }

    /// Reconcile session state on startup.
    ///
    /// For each session record from the previous run:
    /// - Suspended records → set `ConnectionState::Suspended`
    /// - Live SSH PIDs → set `Connected`, adopt with a polling supervisor
    /// - Dead PIDs with `auto_restart` → initiate new connection
    /// - Dead PIDs without `auto_restart` → set `Disconnected`, remove record
    /// - Stale records (UUID not in config) → remove
    fn reconcile_sessions(&mut self) {
        let entry_ids: std::collections::HashSet<Uuid> =
            self.entries.iter().map(|e| e.id).collect();

        // Collect session keys to iterate (avoid borrow issues)
        let session_ids: Vec<Uuid> = self.sessions.keys().cloned().collect();

        // Track which entries to auto-connect after reconciliation
        let mut auto_connect: Vec<usize> = Vec::new();

        for id in session_ids {
            // Remove stale records whose UUID doesn't match any config entry
            if !entry_ids.contains(&id) {
                self.sessions.remove(&id);
                continue;
            }

            let record = match self.sessions.get(&id) {
                Some(r) => r.clone(),
                None => continue,
            };

            if record.suspended {
                // Suspended: set state, no PID check needed
                self.connection_states
                    .insert(id, ConnectionState::Suspended);
                continue;
            }

            if let Some(pid) = record.pid
                && is_live_ssh_tunnel(pid)
            {
                // PID alive and is ssh → adopt it
                self.connection_states
                    .insert(id, ConnectionState::Connected);
                let entry = match self.entries.iter().find(|e| e.id == id) {
                    Some(e) => e.clone(),
                    None => continue,
                };
                let supervisor = Supervisor::adopt(entry, pid, self.tunnel_tx.clone());
                self.supervisors.insert(id, supervisor);
                continue;
            }

            // PID is dead (or no PID recorded) — check auto_restart
            let entry = self.entries.iter().find(|e| e.id == id);
            let auto_restart = entry.is_some_and(|e| e.auto_restart);

            if auto_restart {
                // Will auto-connect after reconciliation loop
                if let Some(idx) = self.entries.iter().position(|e| e.id == id) {
                    auto_connect.push(idx);
                }
                // Remove the stale session record — connect() will create a fresh one
                self.sessions.remove(&id);
            } else {
                // No auto-restart: mark disconnected and remove session
                self.connection_states
                    .insert(id, ConnectionState::Disconnected);
                self.sessions.remove(&id);
            }
        }

        // Auto-connect dead auto_restart tunnels
        for idx in auto_connect {
            self.connect(idx);
        }

        // Save cleaned session state
        self.persist_sessions();
    }

    /// Save the current entries to disk.
    fn save_config(&self) {
        let cfg = Config {
            server: self.entries.clone(),
        };
        if let Err(e) = config::save(&cfg) {
            eprintln!("Failed to save config: {e}");
        }
    }

    /// Persist the session state to disk.
    fn persist_sessions(&self) {
        if let Err(e) = save_sessions(&self.sessions) {
            eprintln!("Failed to save sessions: {e}");
        }
    }

    /// Get a simple ISO 8601-ish UTC timestamp string.
    fn now_timestamp() -> String {
        match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(d) => format!("{}s-since-epoch", d.as_secs()),
            Err(_) => "unknown".to_string(),
        }
    }

    /// Shut down all active supervisors.
    pub fn shutdown(&mut self) {
        for (_id, supervisor) in self.supervisors.drain() {
            supervisor.cancel();
        }
    }
}
