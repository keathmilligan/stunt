//! Core application state and update loop.

use std::collections::HashMap;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use tokio::sync::mpsc;
use uuid::Uuid;

use crate::config::{
    self, Config, K8sEntry, K8sPortForward, K8sResourceType, ServerEntry, SshuttleEntry,
    TunnelEntry, TunnelForward,
};
use crate::config::{load_sessions, save_sessions, SessionRecord, SessionState};
#[cfg(unix)]
use crate::tunnel::is_live_tunnel;
use crate::tunnel::{
    build_kubectl_command, build_ssh_command, build_sshuttle_command, ConnectionState, Supervisor,
    TunnelEvent, TunnelProcessType,
};

/// How long transient status messages are shown (in seconds).
const STATUS_MSG_DURATION_SECS: u64 = 3;

/// The current mode of the application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppMode {
    /// Normal list view.
    Normal,
    /// Choosing the entry type for a new entry (SSH or K8s).
    TypeSelect(EntryTypeSelection),
    /// Editing or creating an entry.
    Form(FormState),
}

/// State for the entry-type selection step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryTypeSelection {
    /// 0 = SSH, 1 = K8s
    pub selected: usize,
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

/// The type of entry being edited in the form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormEntryType {
    /// SSH server entry.
    Ssh,
    /// Kubernetes workload entry.
    K8s,
    /// sshuttle VPN-over-SSH entry.
    Sshuttle,
}

/// State for the entry form (new or edit).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormState {
    /// Which field is currently focused in the server fields section.
    pub focused_field: usize,
    /// Whether we are editing an existing entry (Some(id)) or creating new (None).
    pub editing_id: Option<Uuid>,
    /// The type of entry being edited.
    pub entry_type: FormEntryType,
    /// Field values for SSH: name, host, port, user, identity_file, auto_restart.
    /// Field values for K8s: name, context, namespace, resource_name, auto_restart.
    pub fields: Vec<FormField>,
    /// SSH-only: forwards being edited.
    pub forwards: Vec<TunnelForward>,
    /// Which section of the form currently has focus.
    pub focus: FormFocus,
    /// Currently selected forward index in the list.
    pub selected_forward: usize,
    /// Current forward field being edited (within the forward sub-form).
    pub forward_field: usize,
    /// Forward type selector index (0=Local, 1=Remote, 2=Dynamic) for SSH.
    /// Resource type selector index (0=Pod, 1=Service, 2=Deployment) for K8s.
    pub forward_type: usize,
    /// Forward field values.
    pub forward_fields: Vec<FormField>,
    /// K8s-only: port-forward bindings being edited.
    pub k8s_forwards: Vec<K8sPortForward>,
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
    /// All configured tunnel entries.
    pub entries: Vec<TunnelEntry>,
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
    /// Warning message to show when kubectl is unavailable (persisted until dismissed).
    pub kubectl_warning: Option<String>,
    /// Warning message to show when sshuttle is unavailable (persisted until dismissed).
    pub sshuttle_warning: Option<String>,
    /// Whether the app is running in demo mode (read-only, simulated tunnels).
    pub demo_mode: bool,
}

impl App {
    /// Create a new App from a loaded config, a tunnel event sender, and a demo flag.
    ///
    /// When `demo_mode` is true, session loading and reconciliation are skipped
    /// entirely — no disk I/O occurs.
    pub fn new(
        config: Config,
        tunnel_tx: mpsc::UnboundedSender<TunnelEvent>,
        demo_mode: bool,
    ) -> Self {
        let mut connection_states = HashMap::new();
        for entry in &config.entries {
            connection_states.insert(entry.id(), ConnectionState::Disconnected);
        }

        let sessions = if demo_mode {
            SessionState::new()
        } else {
            load_sessions()
        };

        let mut app = App {
            entries: config.entries,
            selected: 0,
            scroll_offset: 0,
            connection_states,
            supervisors: HashMap::new(),
            sessions,
            tunnel_tx,
            status_message: None,
            running: true,
            mode: AppMode::Normal,
            kubectl_warning: None,
            sshuttle_warning: None,
            demo_mode,
        };

        if !demo_mode {
            app.reconcile_sessions();
        }
        app
    }

    /// Set a kubectl availability warning (shown in status bar).
    pub fn set_kubectl_warning(&mut self, msg: impl Into<String>) {
        self.kubectl_warning = Some(msg.into());
    }

    /// Set an sshuttle availability warning (shown in status bar).
    pub fn set_sshuttle_warning(&mut self, msg: impl Into<String>) {
        self.sshuttle_warning = Some(msg.into());
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

    /// Show a read-only warning for demo mode.
    pub fn set_demo_readonly_message(&mut self) {
        self.set_status("Demo mode \u{2014} read-only");
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
            AppMode::TypeSelect(_) => self.update_type_select(msg),
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
                // Show type-selection step before opening the full form
                self.mode = AppMode::TypeSelect(EntryTypeSelection { selected: 0 });
            }
            Message::EditEntry => {
                if let Some(entry) = self.entries.get(self.selected) {
                    match entry {
                        TunnelEntry::Ssh(e) => {
                            self.mode = AppMode::Form(Self::ssh_entry_to_form(e));
                        }
                        TunnelEntry::K8s(e) => {
                            self.mode = AppMode::Form(Self::k8s_entry_to_form(e));
                        }
                        TunnelEntry::Sshuttle(e) => {
                            self.mode = AppMode::Form(Self::sshuttle_entry_to_form(e));
                        }
                    }
                }
            }
            Message::DeleteEntry => {
                if let Some(entry) = self.entries.get(self.selected) {
                    let id = entry.id();
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
                    let id = entry.id();
                    let name = entry.name().to_string();
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

    /// Handle messages in type-selection mode.
    fn update_type_select(&mut self, msg: Message) {
        let sel = match &mut self.mode {
            AppMode::TypeSelect(s) => s,
            _ => return,
        };

        match msg {
            Message::NavigateUp | Message::FormPrevField => {
                if sel.selected > 0 {
                    sel.selected -= 1;
                }
            }
            Message::NavigateDown | Message::FormNextField => {
                if sel.selected < 2 {
                    sel.selected += 1;
                }
            }
            Message::FormCycleForwardType => {
                // Also cycle on Ctrl+T
                let s = match &mut self.mode {
                    AppMode::TypeSelect(s) => s,
                    _ => return,
                };
                s.selected = (s.selected + 1) % 3;
            }
            Message::FormSubmit => {
                let entry_type_idx = match &self.mode {
                    AppMode::TypeSelect(s) => s.selected,
                    _ => return,
                };
                // Transition to the appropriate form
                match entry_type_idx {
                    0 => self.mode = AppMode::Form(Self::new_ssh_form()),
                    1 => self.mode = AppMode::Form(Self::new_k8s_form()),
                    _ => self.mode = AppMode::Form(Self::new_sshuttle_form()),
                }
            }
            Message::FormCancel | Message::Quit => {
                self.mode = AppMode::Normal;
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
                match form.entry_type {
                    FormEntryType::Sshuttle => {
                        // Sshuttle form: only server fields, no forwards sub-form
                        if form.focused_field < form.fields.len() - 1 {
                            form.focused_field += 1;
                        }
                    }
                    FormEntryType::K8s => {
                        // K8s form: simple field cycling + k8s forward list
                        match form.focus {
                            FormFocus::ServerFields => {
                                if form.focused_field < form.fields.len() - 1 {
                                    form.focused_field += 1;
                                } else if !form.k8s_forwards.is_empty() {
                                    form.focus = FormFocus::ForwardList;
                                    form.selected_forward = 0;
                                }
                            }
                            FormFocus::ForwardList => {
                                if form.selected_forward < form.k8s_forwards.len().saturating_sub(1)
                                {
                                    form.selected_forward += 1;
                                }
                            }
                            FormFocus::ForwardEdit { .. } => {
                                if form.forward_field < 1 {
                                    form.forward_field += 1;
                                }
                            }
                        }
                    }
                    FormEntryType::Ssh => match form.focus {
                        FormFocus::ServerFields => {
                            if form.focused_field < form.fields.len() - 1 {
                                form.focused_field += 1;
                            } else if !form.forwards.is_empty() {
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
                    },
                }
            }
            Message::FormPrevField => match form.entry_type {
                FormEntryType::Sshuttle => {
                    if form.focused_field > 0 {
                        form.focused_field -= 1;
                    }
                }
                FormEntryType::K8s => match form.focus {
                    FormFocus::ServerFields => {
                        if form.focused_field > 0 {
                            form.focused_field -= 1;
                        }
                    }
                    FormFocus::ForwardList => {
                        if form.selected_forward > 0 {
                            form.selected_forward -= 1;
                        } else {
                            form.focus = FormFocus::ServerFields;
                            form.focused_field = form.fields.len() - 1;
                        }
                    }
                    FormFocus::ForwardEdit { .. } => {
                        if form.forward_field > 0 {
                            form.forward_field -= 1;
                        }
                    }
                },
                FormEntryType::Ssh => match form.focus {
                    FormFocus::ServerFields => {
                        if form.focused_field > 0 {
                            form.focused_field -= 1;
                        }
                    }
                    FormFocus::ForwardList => {
                        if form.selected_forward > 0 {
                            form.selected_forward -= 1;
                        } else {
                            form.focus = FormFocus::ServerFields;
                            form.focused_field = form.fields.len() - 1;
                        }
                    }
                    FormFocus::ForwardEdit { .. } => {
                        if form.forward_field > 0 {
                            form.forward_field -= 1;
                        }
                    }
                },
            },
            Message::FormInput(ch) => {
                match form.focus {
                    FormFocus::ServerFields => {
                        if let Some(field) = form.fields.get_mut(form.focused_field) {
                            if field.label == "Auto Restart" {
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
            Message::FormBackspace => match form.focus {
                FormFocus::ServerFields => {
                    if let Some(field) = form.fields.get_mut(form.focused_field) {
                        if field.label == "Auto Restart" {
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
                FormFocus::ForwardList => {}
                FormFocus::ForwardEdit { .. } => {
                    if let Some(field) = form.forward_fields.get_mut(form.forward_field) {
                        field.value.pop();
                    }
                }
            },
            Message::FormAddForward => match form.entry_type {
                FormEntryType::Sshuttle => { /* sshuttle has no forwards */ }
                FormEntryType::Ssh => match form.focus {
                    FormFocus::ForwardEdit { .. } => {
                        if let Some(fwd) = self.build_ssh_forward_from_form() {
                            self.commit_ssh_forward(fwd);
                            let form = match &mut self.mode {
                                AppMode::Form(f) => f,
                                _ => return,
                            };
                            form.focus = FormFocus::ForwardEdit {
                                editing_index: None,
                            };
                            form.forward_field = 0;
                            form.forward_type = 0;
                            form.forward_fields = Self::default_ssh_forward_fields();
                        }
                    }
                    _ => {
                        form.focus = FormFocus::ForwardEdit {
                            editing_index: None,
                        };
                        form.forward_field = 0;
                        form.forward_type = 0;
                        form.forward_fields = Self::default_ssh_forward_fields();
                    }
                },
                FormEntryType::K8s => match form.focus {
                    FormFocus::ForwardEdit { .. } => {
                        if let Some(fwd) = self.build_k8s_forward_from_form() {
                            self.commit_k8s_forward(fwd);
                            let form = match &mut self.mode {
                                AppMode::Form(f) => f,
                                _ => return,
                            };
                            form.focus = FormFocus::ForwardEdit {
                                editing_index: None,
                            };
                            form.forward_field = 0;
                            form.forward_fields = Self::default_k8s_forward_fields();
                        }
                    }
                    _ => {
                        form.focus = FormFocus::ForwardEdit {
                            editing_index: None,
                        };
                        form.forward_field = 0;
                        form.forward_fields = Self::default_k8s_forward_fields();
                    }
                },
            },
            Message::FormDeleteForward => match form.focus {
                FormFocus::ForwardEdit { .. } => match form.entry_type {
                    FormEntryType::Sshuttle => { /* no forwards */ }
                    FormEntryType::Ssh => {
                        if form.forwards.is_empty() {
                            form.focus = FormFocus::ServerFields;
                        } else {
                            form.selected_forward = form
                                .selected_forward
                                .min(form.forwards.len().saturating_sub(1));
                            form.focus = FormFocus::ForwardList;
                        }
                    }
                    FormEntryType::K8s => {
                        if form.k8s_forwards.is_empty() {
                            form.focus = FormFocus::ServerFields;
                        } else {
                            form.selected_forward = form
                                .selected_forward
                                .min(form.k8s_forwards.len().saturating_sub(1));
                            form.focus = FormFocus::ForwardList;
                        }
                    }
                },
                FormFocus::ForwardList => match form.entry_type {
                    FormEntryType::Sshuttle => { /* no forwards */ }
                    FormEntryType::Ssh => {
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
                    FormEntryType::K8s => {
                        if !form.k8s_forwards.is_empty() {
                            form.k8s_forwards.remove(form.selected_forward);
                            if form.k8s_forwards.is_empty() {
                                form.focus = FormFocus::ServerFields;
                                form.focused_field = form.fields.len() - 1;
                            } else {
                                form.selected_forward = form
                                    .selected_forward
                                    .min(form.k8s_forwards.len().saturating_sub(1));
                            }
                        }
                    }
                },
                FormFocus::ServerFields => {}
            },
            Message::FormCycleForwardType => {
                if matches!(form.focus, FormFocus::ForwardEdit { .. }) {
                    match form.entry_type {
                        FormEntryType::Sshuttle => { /* no forward types */ }
                        FormEntryType::Ssh => {
                            form.forward_type = (form.forward_type + 1) % 3;
                            form.forward_fields = Self::default_ssh_forward_fields();
                            form.forward_field = 0;
                        }
                        FormEntryType::K8s => {
                            // K8s resource type cycling (pod/service/deployment) is handled
                            // in the server fields (a dedicated field), not in forward sub-form.
                        }
                    }
                }
            }
            Message::FormSubmit => {
                match self.mode {
                    AppMode::Form(ref form)
                        if matches!(form.focus, FormFocus::ForwardEdit { .. }) =>
                    {
                        match form.entry_type {
                            FormEntryType::Sshuttle => { /* no forwards */ }
                            FormEntryType::Ssh => {
                                if let Some(fwd) = self.build_ssh_forward_from_form() {
                                    self.commit_ssh_forward(fwd);
                                    let form = match &mut self.mode {
                                        AppMode::Form(f) => f,
                                        _ => return,
                                    };
                                    form.selected_forward = form
                                        .selected_forward
                                        .min(form.forwards.len().saturating_sub(1));
                                    form.focus = FormFocus::ForwardList;
                                }
                            }
                            FormEntryType::K8s => {
                                if let Some(fwd) = self.build_k8s_forward_from_form() {
                                    self.commit_k8s_forward(fwd);
                                    let form = match &mut self.mode {
                                        AppMode::Form(f) => f,
                                        _ => return,
                                    };
                                    form.selected_forward = form
                                        .selected_forward
                                        .min(form.k8s_forwards.len().saturating_sub(1));
                                    form.focus = FormFocus::ForwardList;
                                }
                            }
                        }
                        return;
                    }
                    AppMode::Form(ref form) if matches!(form.focus, FormFocus::ForwardList) => {
                        match form.entry_type {
                            FormEntryType::Sshuttle => { /* no forwards */ }
                            FormEntryType::Ssh => {
                                let idx = form.selected_forward;
                                if let Some(fwd) = form.forwards.get(idx) {
                                    let (ftype, fields) = Self::ssh_forward_to_fields(fwd);
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
                            }
                            FormEntryType::K8s => {
                                let idx = form.selected_forward;
                                if let Some(fwd) = form.k8s_forwards.get(idx) {
                                    let fields = Self::k8s_forward_to_fields(fwd);
                                    let form = match &mut self.mode {
                                        AppMode::Form(f) => f,
                                        _ => return,
                                    };
                                    form.focus = FormFocus::ForwardEdit {
                                        editing_index: Some(idx),
                                    };
                                    form.forward_fields = fields;
                                    form.forward_field = 0;
                                }
                            }
                        }
                        return;
                    }
                    _ => {}
                }
                self.submit_form();
            }
            Message::FormCancel => {
                let form = match &mut self.mode {
                    AppMode::Form(f) => f,
                    _ => return,
                };
                match form.focus {
                    FormFocus::ForwardEdit { .. } => {
                        let is_empty = match form.entry_type {
                            FormEntryType::Sshuttle => true,
                            FormEntryType::Ssh => form.forwards.is_empty(),
                            FormEntryType::K8s => form.k8s_forwards.is_empty(),
                        };
                        if is_empty {
                            form.focus = FormFocus::ServerFields;
                        } else {
                            let len = match form.entry_type {
                                FormEntryType::Sshuttle => 0,
                                FormEntryType::Ssh => form.forwards.len(),
                                FormEntryType::K8s => form.k8s_forwards.len(),
                            };
                            form.selected_forward =
                                form.selected_forward.min(len.saturating_sub(1));
                            form.focus = FormFocus::ForwardList;
                        }
                    }
                    _ => {
                        self.mode = AppMode::Normal;
                    }
                }
            }
            Message::TunnelEvent(event) => {
                self.handle_tunnel_event(event);
            }
            _ => {}
        }
    }

    // ── Form builders ──────────────────────────────────────────────────────

    /// Build a new empty SSH entry form.
    fn new_ssh_form() -> FormState {
        FormState {
            focused_field: 0,
            editing_id: None,
            entry_type: FormEntryType::Ssh,
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
            forward_fields: Self::default_ssh_forward_fields(),
            k8s_forwards: vec![],
        }
    }

    /// Build a new empty K8s entry form.
    fn new_k8s_form() -> FormState {
        FormState {
            focused_field: 0,
            editing_id: None,
            entry_type: FormEntryType::K8s,
            fields: vec![
                FormField {
                    label: "Name".to_string(),
                    value: String::new(),
                },
                FormField {
                    label: "Context".to_string(),
                    value: String::new(),
                },
                FormField {
                    label: "Namespace".to_string(),
                    value: String::new(),
                },
                FormField {
                    label: "Resource Type".to_string(),
                    value: "deployment".to_string(),
                },
                FormField {
                    label: "Resource Name".to_string(),
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
            forward_fields: Self::default_k8s_forward_fields(),
            k8s_forwards: vec![],
        }
    }

    /// Build a new empty sshuttle entry form.
    fn new_sshuttle_form() -> FormState {
        FormState {
            focused_field: 0,
            editing_id: None,
            entry_type: FormEntryType::Sshuttle,
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
                    label: "Subnets".to_string(),
                    value: String::new(),
                },
                FormField {
                    label: "Port".to_string(),
                    value: String::new(),
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
            forward_fields: vec![],
            k8s_forwards: vec![],
        }
    }

    /// Build a form pre-populated from an existing sshuttle entry.
    fn sshuttle_entry_to_form(entry: &SshuttleEntry) -> FormState {
        FormState {
            focused_field: 0,
            editing_id: Some(entry.id),
            entry_type: FormEntryType::Sshuttle,
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
                    label: "Subnets".to_string(),
                    value: entry.subnets.join(", "),
                },
                FormField {
                    label: "Port".to_string(),
                    value: entry.port.map(|p| p.to_string()).unwrap_or_default(),
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
            forwards: vec![],
            focus: FormFocus::ServerFields,
            selected_forward: 0,
            forward_field: 0,
            forward_type: 0,
            forward_fields: vec![],
            k8s_forwards: vec![],
        }
    }

    /// Build a form pre-populated from an existing SSH entry.
    fn ssh_entry_to_form(entry: &ServerEntry) -> FormState {
        FormState {
            focused_field: 0,
            editing_id: Some(entry.id),
            entry_type: FormEntryType::Ssh,
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
            forward_fields: Self::default_ssh_forward_fields(),
            k8s_forwards: vec![],
        }
    }

    /// Build a form pre-populated from an existing K8s entry.
    fn k8s_entry_to_form(entry: &K8sEntry) -> FormState {
        FormState {
            focused_field: 0,
            editing_id: Some(entry.id),
            entry_type: FormEntryType::K8s,
            fields: vec![
                FormField {
                    label: "Name".to_string(),
                    value: entry.name.clone(),
                },
                FormField {
                    label: "Context".to_string(),
                    value: entry.context.clone().unwrap_or_default(),
                },
                FormField {
                    label: "Namespace".to_string(),
                    value: entry.namespace.clone().unwrap_or_default(),
                },
                FormField {
                    label: "Resource Type".to_string(),
                    value: entry.resource_type.as_str().to_string(),
                },
                FormField {
                    label: "Resource Name".to_string(),
                    value: entry.resource_name.clone(),
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
            forwards: vec![],
            focus: FormFocus::ServerFields,
            selected_forward: 0,
            forward_field: 0,
            forward_type: 0,
            forward_fields: Self::default_k8s_forward_fields(),
            k8s_forwards: entry.forwards.clone(),
        }
    }

    // ── SSH forward helpers ────────────────────────────────────────────────

    /// Commit a built SSH forward to the form.
    fn commit_ssh_forward(&mut self, fwd: TunnelForward) {
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
            _ => {
                form.forwards.push(fwd);
                form.selected_forward = form.forwards.len() - 1;
            }
        }
    }

    /// Extract SSH forward type index and form fields from an existing forward.
    fn ssh_forward_to_fields(fwd: &TunnelForward) -> (usize, Vec<FormField>) {
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

    /// Build a `TunnelForward` from the current form state.
    fn build_ssh_forward_from_form(&self) -> Option<TunnelForward> {
        let form = match &self.mode {
            AppMode::Form(f) => f,
            _ => return None,
        };

        let bind_port: u16 = form.forward_fields.first()?.value.parse().ok()?;

        match form.forward_type {
            0 => {
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
            2 => Some(TunnelForward::Dynamic {
                bind_address: "127.0.0.1".to_string(),
                bind_port,
            }),
            _ => None,
        }
    }

    /// Default fields for the SSH forward sub-form.
    fn default_ssh_forward_fields() -> Vec<FormField> {
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

    // ── K8s forward helpers ────────────────────────────────────────────────

    /// Commit a built K8s forward to the form.
    fn commit_k8s_forward(&mut self, fwd: K8sPortForward) {
        let form = match &mut self.mode {
            AppMode::Form(f) => f,
            _ => return,
        };
        match form.focus {
            FormFocus::ForwardEdit {
                editing_index: Some(idx),
            } => {
                if idx < form.k8s_forwards.len() {
                    form.k8s_forwards[idx] = fwd;
                    form.selected_forward = idx;
                } else {
                    form.k8s_forwards.push(fwd);
                    form.selected_forward = form.k8s_forwards.len() - 1;
                }
            }
            _ => {
                form.k8s_forwards.push(fwd);
                form.selected_forward = form.k8s_forwards.len() - 1;
            }
        }
    }

    /// Extract K8s forward form fields from an existing binding.
    fn k8s_forward_to_fields(fwd: &K8sPortForward) -> Vec<FormField> {
        vec![
            FormField {
                label: "Local Port".to_string(),
                value: fwd.local_port.to_string(),
            },
            FormField {
                label: "Remote Port".to_string(),
                value: fwd.remote_port.to_string(),
            },
        ]
    }

    /// Build a `K8sPortForward` from the current form state.
    fn build_k8s_forward_from_form(&self) -> Option<K8sPortForward> {
        let form = match &self.mode {
            AppMode::Form(f) => f,
            _ => return None,
        };

        let local_port: u16 = form.forward_fields.first()?.value.parse().ok()?;
        let remote_port: u16 = form.forward_fields.get(1)?.value.parse().ok()?;

        Some(K8sPortForward {
            local_bind_address: "127.0.0.1".to_string(),
            local_port,
            remote_port,
        })
    }

    /// Default fields for the K8s forward sub-form.
    fn default_k8s_forward_fields() -> Vec<FormField> {
        vec![
            FormField {
                label: "Local Port".to_string(),
                value: String::new(),
            },
            FormField {
                label: "Remote Port".to_string(),
                value: String::new(),
            },
        ]
    }

    // ── Form submission ────────────────────────────────────────────────────

    /// Submit the form and create/update the entry.
    fn submit_form(&mut self) {
        let form = match &self.mode {
            AppMode::Form(f) => f.clone(),
            _ => return,
        };

        match form.entry_type {
            FormEntryType::Ssh => self.submit_ssh_form(form),
            FormEntryType::K8s => self.submit_k8s_form(form),
            FormEntryType::Sshuttle => self.submit_sshuttle_form(form),
        }
    }

    /// Submit an SSH entry form.
    fn submit_ssh_form(&mut self, form: FormState) {
        let name = form.fields[0].value.trim().to_string();
        let host = form.fields[1].value.trim().to_string();
        let port: u16 = form.fields[2].value.trim().parse().unwrap_or(22);
        let user = {
            let val = form.fields[3].value.trim().to_string();
            if val.is_empty() {
                None
            } else {
                Some(val)
            }
        };
        let identity_file = {
            let val = form.fields[4].value.trim().to_string();
            if val.is_empty() {
                None
            } else {
                Some(val)
            }
        };
        let auto_restart = form.fields[5].value.trim().eq_ignore_ascii_case("yes");

        if name.is_empty() || host.is_empty() {
            self.set_status("Name and Host are required");
            return;
        }

        if let Some(id) = form.editing_id {
            if let Some(entry) = self.entries.iter_mut().find(|e| e.id() == id)
                && let TunnelEntry::Ssh(ssh) = entry
            {
                ssh.name = name;
                ssh.host = host;
                ssh.port = port;
                ssh.user = user;
                ssh.identity_file = identity_file;
                ssh.forwards = form.forwards;
                ssh.auto_restart = auto_restart;
            }
            self.set_status("Entry updated");
        } else {
            let id = Uuid::new_v4();
            let entry = TunnelEntry::Ssh(ServerEntry {
                id,
                name,
                host,
                port,
                user,
                identity_file,
                forwards: form.forwards,
                auto_restart,
            });
            self.entries.push(entry);
            self.connection_states
                .insert(id, ConnectionState::Disconnected);
            self.selected = self.entries.len() - 1;
            self.set_status("Entry created");
        }

        self.save_config();
        self.mode = AppMode::Normal;
    }

    /// Submit a K8s entry form.
    fn submit_k8s_form(&mut self, form: FormState) {
        let name = form.fields[0].value.trim().to_string();
        let context = {
            let val = form.fields[1].value.trim().to_string();
            if val.is_empty() {
                None
            } else {
                Some(val)
            }
        };
        let namespace = {
            let val = form.fields[2].value.trim().to_string();
            if val.is_empty() {
                None
            } else {
                Some(val)
            }
        };
        let resource_type_str = form.fields[3].value.trim().to_lowercase();
        let resource_type = match resource_type_str.as_str() {
            "pod" => K8sResourceType::Pod,
            "service" => K8sResourceType::Service,
            _ => K8sResourceType::Deployment,
        };
        let resource_name = form.fields[4].value.trim().to_string();
        let auto_restart = form.fields[5].value.trim().eq_ignore_ascii_case("yes");

        if name.is_empty() || resource_name.is_empty() {
            self.set_status("Name and Resource Name are required");
            return;
        }

        if let Some(id) = form.editing_id {
            if let Some(entry) = self.entries.iter_mut().find(|e| e.id() == id)
                && let TunnelEntry::K8s(k8s) = entry
            {
                k8s.name = name;
                k8s.context = context;
                k8s.namespace = namespace;
                k8s.resource_type = resource_type;
                k8s.resource_name = resource_name;
                k8s.forwards = form.k8s_forwards;
                k8s.auto_restart = auto_restart;
            }
            self.set_status("Entry updated");
        } else {
            let id = Uuid::new_v4();
            let entry = TunnelEntry::K8s(K8sEntry {
                id,
                name,
                context,
                namespace,
                resource_type,
                resource_name,
                forwards: form.k8s_forwards,
                auto_restart,
            });
            self.entries.push(entry);
            self.connection_states
                .insert(id, ConnectionState::Disconnected);
            self.selected = self.entries.len() - 1;
            self.set_status("Entry created");
        }

        self.save_config();
        self.mode = AppMode::Normal;
    }

    /// Submit a sshuttle entry form.
    fn submit_sshuttle_form(&mut self, form: FormState) {
        let name = form.fields[0].value.trim().to_string();
        let host = form.fields[1].value.trim().to_string();
        let subnets: Vec<String> = form.fields[2]
            .value
            .split([',', '\n'])
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        let port = {
            let val = form.fields[3].value.trim().to_string();
            if val.is_empty() {
                None
            } else {
                val.parse::<u16>().ok()
            }
        };
        let user = {
            let val = form.fields[4].value.trim().to_string();
            if val.is_empty() {
                None
            } else {
                Some(val)
            }
        };
        let identity_file = {
            let val = form.fields[5].value.trim().to_string();
            if val.is_empty() {
                None
            } else {
                Some(val)
            }
        };
        let auto_restart = form.fields[6].value.trim().eq_ignore_ascii_case("yes");

        if name.is_empty() || host.is_empty() {
            self.set_status("Name and Host are required");
            return;
        }
        if subnets.is_empty() {
            self.set_status("At least one subnet is required");
            return;
        }

        if let Some(id) = form.editing_id {
            if let Some(entry) = self.entries.iter_mut().find(|e| e.id() == id)
                && let TunnelEntry::Sshuttle(s) = entry
            {
                s.name = name;
                s.host = host;
                s.subnets = subnets;
                s.port = port;
                s.user = user;
                s.identity_file = identity_file;
                s.auto_restart = auto_restart;
            }
            self.set_status("Entry updated");
        } else {
            let id = Uuid::new_v4();
            let entry = TunnelEntry::Sshuttle(SshuttleEntry {
                id,
                name,
                host,
                port,
                user,
                identity_file,
                subnets,
                auto_restart,
            });
            self.entries.push(entry);
            self.connection_states
                .insert(id, ConnectionState::Disconnected);
            self.selected = self.entries.len() - 1;
            self.set_status("Entry created");
        }

        self.save_config();
        self.mode = AppMode::Normal;
    }

    // ── Connection management ──────────────────────────────────────────────

    /// Initiate a connection for the entry at the given index.
    fn connect(&mut self, index: usize) {
        let entry = match self.entries.get(index) {
            Some(e) => e.clone(),
            None => return,
        };
        let id = entry.id();

        self.connection_states
            .insert(id, ConnectionState::Connecting);

        let supervisor = match &entry {
            TunnelEntry::Ssh(ssh_entry) => {
                let ssh_entry = ssh_entry.clone();
                Supervisor::spawn(
                    id,
                    ssh_entry.auto_restart,
                    TunnelProcessType::Ssh,
                    Box::new(move || build_ssh_command(&ssh_entry)),
                    self.tunnel_tx.clone(),
                )
            }
            TunnelEntry::K8s(k8s_entry) => {
                // Spawn one supervisor per K8s port-forward binding.
                // If there are no forwards yet, spawn a single connection using an empty
                // kubectl command (which will fail quickly) so the user sees a Failed state.
                if k8s_entry.forwards.is_empty() {
                    // No bindings — use the first forward (none) for a placeholder supervisor
                    // that will immediately fail. The user must add a forward binding first.
                    self.set_status("No port-forward bindings configured");
                    self.connection_states.insert(id, ConnectionState::Failed);
                    return;
                }
                // Spawn for the first forward only; additional forwards would need
                // separate supervisor tracking — deferred to future multi-forward support.
                let forward = k8s_entry.forwards[0].clone();
                let k8s_entry = k8s_entry.clone();
                Supervisor::spawn(
                    id,
                    k8s_entry.auto_restart,
                    TunnelProcessType::Kubectl,
                    Box::new(move || build_kubectl_command(&k8s_entry, &forward)),
                    self.tunnel_tx.clone(),
                )
            }
            TunnelEntry::Sshuttle(sshuttle_entry) => {
                let sshuttle_entry = sshuttle_entry.clone();
                Supervisor::spawn(
                    id,
                    sshuttle_entry.auto_restart,
                    TunnelProcessType::Sshuttle,
                    Box::new(move || build_sshuttle_command(&sshuttle_entry)),
                    self.tunnel_tx.clone(),
                )
            }
        };

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
            .find(|e| e.id() == id)
            .is_some_and(|e| e.auto_restart());

        if let Some(supervisor) = self.supervisors.remove(&id) {
            supervisor.set_suspended(true);
            supervisor.cancel_and_kill();
        }

        if auto_restart {
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

                if let Some(record) = self.sessions.get_mut(&entry_id) {
                    record.connected_at = Some(Self::now_timestamp());
                }
                self.persist_sessions();

                if let Some(entry) = self.entries.iter().find(|e| e.id() == entry_id) {
                    self.set_status(format!("Connected: {}", entry.name()));
                }
            }
            TunnelEvent::Disconnected { entry_id } => {
                let auto_restart = self
                    .entries
                    .iter()
                    .find(|e| e.id() == entry_id)
                    .is_some_and(|e| e.auto_restart());

                if auto_restart {
                    self.connection_states
                        .insert(entry_id, ConnectionState::Failed);
                    if let Some(entry) = self.entries.iter().find(|e| e.id() == entry_id) {
                        self.set_status(format!("Connection lost: {}", entry.name()));
                    }
                } else {
                    self.connection_states
                        .insert(entry_id, ConnectionState::Failed);
                    self.supervisors.remove(&entry_id);
                    self.sessions.remove(&entry_id);
                    self.persist_sessions();
                    if let Some(entry) = self.entries.iter().find(|e| e.id() == entry_id) {
                        self.set_status(format!("Connection lost: {}", entry.name()));
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
                if let Some(entry) = self.entries.iter().find(|e| e.id() == entry_id) {
                    self.set_status(format!(
                        "Reconnecting {} (attempt {}, {}s delay)",
                        entry.name(),
                        attempt,
                        delay_secs
                    ));
                }
            }
            TunnelEvent::PidUpdate { entry_id, pid } => {
                // In demo mode, PidUpdate signals the start of a connection attempt.
                // Transition to Connecting so the TUI shows the intermediate state.
                if self.demo_mode {
                    let current = self.state_of(&entry_id);
                    if matches!(
                        current,
                        ConnectionState::Disconnected | ConnectionState::Failed
                    ) {
                        self.connection_states
                            .insert(entry_id, ConnectionState::Connecting);
                    }
                }

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
    fn reconcile_sessions(&mut self) {
        let entry_ids: std::collections::HashSet<Uuid> =
            self.entries.iter().map(|e| e.id()).collect();

        let session_ids: Vec<Uuid> = self.sessions.keys().cloned().collect();
        let mut auto_connect: Vec<usize> = Vec::new();

        for id in session_ids {
            if !entry_ids.contains(&id) {
                self.sessions.remove(&id);
                continue;
            }

            let record = match self.sessions.get(&id) {
                Some(r) => r.clone(),
                None => continue,
            };

            if record.suspended {
                self.connection_states
                    .insert(id, ConnectionState::Suspended);
                continue;
            }

            // Determine process type for liveness check (used on unix only)
            let _process_type = self
                .entries
                .iter()
                .find(|e| e.id() == id)
                .map(|e| match e {
                    TunnelEntry::Ssh(_) => TunnelProcessType::Ssh,
                    TunnelEntry::K8s(_) => TunnelProcessType::Kubectl,
                    TunnelEntry::Sshuttle(_) => TunnelProcessType::Sshuttle,
                })
                .unwrap_or(TunnelProcessType::Ssh);

            #[cfg(unix)]
            if let Some(pid) = record.pid
                && is_live_tunnel(pid, _process_type)
            {
                // PID alive — adopt it
                self.connection_states
                    .insert(id, ConnectionState::Connected);
                let entry = match self.entries.iter().find(|e| e.id() == id) {
                    Some(e) => e.clone(),
                    None => continue,
                };
                let auto_restart = entry.auto_restart();

                let supervisor = match &entry {
                    TunnelEntry::Ssh(ssh) => {
                        let ssh = ssh.clone();
                        Supervisor::adopt(
                            id,
                            pid,
                            auto_restart,
                            TunnelProcessType::Ssh,
                            Box::new(move || build_ssh_command(&ssh)),
                            self.tunnel_tx.clone(),
                        )
                    }
                    TunnelEntry::K8s(k8s) => {
                        if k8s.forwards.is_empty() {
                            continue;
                        }
                        let forward = k8s.forwards[0].clone();
                        let k8s = k8s.clone();
                        Supervisor::adopt(
                            id,
                            pid,
                            auto_restart,
                            TunnelProcessType::Kubectl,
                            Box::new(move || build_kubectl_command(&k8s, &forward)),
                            self.tunnel_tx.clone(),
                        )
                    }
                    TunnelEntry::Sshuttle(sshuttle) => {
                        let sshuttle = sshuttle.clone();
                        Supervisor::adopt(
                            id,
                            pid,
                            auto_restart,
                            TunnelProcessType::Sshuttle,
                            Box::new(move || build_sshuttle_command(&sshuttle)),
                            self.tunnel_tx.clone(),
                        )
                    }
                };
                self.supervisors.insert(id, supervisor);
                continue;
            }

            // PID is dead (or no PID recorded) — check auto_restart
            let auto_restart = self
                .entries
                .iter()
                .find(|e| e.id() == id)
                .is_some_and(|e| e.auto_restart());

            if auto_restart {
                if let Some(idx) = self.entries.iter().position(|e| e.id() == id) {
                    auto_connect.push(idx);
                }
                self.sessions.remove(&id);
            } else {
                self.connection_states
                    .insert(id, ConnectionState::Disconnected);
                self.sessions.remove(&id);
            }
        }

        for idx in auto_connect {
            self.connect(idx);
        }

        self.persist_sessions();
    }

    /// Save the current entries to disk.
    ///
    /// In demo mode this is a no-op — config is never persisted.
    fn save_config(&self) {
        if self.demo_mode {
            return;
        }
        let cfg = Config {
            entries: self.entries.clone(),
        };
        if let Err(e) = config::save(&cfg) {
            eprintln!("Failed to save config: {e}");
        }
    }

    /// Persist the session state to disk.
    ///
    /// In demo mode this is a no-op — no session state is written.
    fn persist_sessions(&self) {
        if self.demo_mode {
            return;
        }
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
