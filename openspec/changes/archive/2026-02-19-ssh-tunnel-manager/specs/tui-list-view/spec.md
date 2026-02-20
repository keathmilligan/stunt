## ADDED Requirements

### Requirement: Three-region screen layout
The system SHALL divide the terminal screen into three vertical regions using ratatui Layout constraints: a title bar (1 line, fixed height at top), a list viewport (remaining space in the middle), and a status bar (1 line, fixed height at bottom).

#### Scenario: Terminal with 40 rows
- **WHEN** the terminal has 40 rows
- **THEN** the title bar occupies row 0, the status bar occupies row 39, and the list viewport occupies rows 1-38

#### Scenario: Terminal resize
- **WHEN** the terminal is resized
- **THEN** the layout recalculates and the list viewport adjusts to fill the remaining space between the fixed-height bars

### Requirement: Title bar with app name and key hints
The system SHALL render a title bar displaying the application name ("tunnel-mgr") left-aligned and key binding hints right-aligned (e.g., `[n]ew  [e]dit  [d]elete  [Enter] connect  [q]uit`). The title bar SHALL use bold text on a colored background to visually anchor the top of the screen.

#### Scenario: Title bar content
- **WHEN** the app renders the title bar
- **THEN** "tunnel-mgr" appears on the left and key binding hints appear on the right within the same line

### Requirement: Status bar with connection summary
The system SHALL render a status bar displaying: the total number of server entries, the count of connected entries, the count of failed entries, and any transient message (e.g., "Saved", "Connection failed"). The status bar SHALL use a distinct background color to visually separate it from the list.

#### Scenario: Status bar counts
- **WHEN** there are 5 server entries, 3 connected, 1 failed
- **THEN** the status bar displays text indicating "5 entries  3 connected  1 failed"

#### Scenario: Transient message display
- **WHEN** the user saves a configuration change
- **THEN** the status bar temporarily shows "Saved" for a short duration before reverting to the default summary

### Requirement: Multi-line server entry rows
Each server entry SHALL be rendered as a multi-line row in the list viewport. The first line SHALL display the server name, connection state indicator, host, port, user, and identity file. Subsequent lines SHALL each display one tunnel forward definition with its type label and addressing details. Row height SHALL vary based on the number of forwards (minimum 2 lines: header + at least one blank/separator line).

#### Scenario: Server with two forwards
- **WHEN** a server entry has name "prod-db", host "bastion.example.com", user "deploy", and two local forwards
- **THEN** the row renders as 3+ lines: one header line and one line per forward

#### Scenario: Server with no forwards
- **WHEN** a server entry has no tunnel forwards defined
- **THEN** the row renders with the header line and a minimum height (no forward lines)

### Requirement: Variable-height row scrolling
The list viewport SHALL scroll by whole rows when the user navigates up or down. The scroll position SHALL be tracked as a row index (not a line offset). The viewport SHALL adjust to keep the selected row fully visible, scrolling up or down as needed when the selected row would be partially or fully outside the visible area.

#### Scenario: Scroll down past viewport
- **WHEN** the selected row is below the visible area of the viewport
- **THEN** the viewport scrolls down until the selected row is fully visible at the bottom of the viewport

#### Scenario: Scroll up past viewport
- **WHEN** the selected row is above the visible area of the viewport
- **THEN** the viewport scrolls up until the selected row is fully visible at the top of the viewport

#### Scenario: Selected row fits in viewport
- **WHEN** the selected row is already fully visible within the viewport
- **THEN** the scroll position does not change

### Requirement: Row selection highlight
The currently selected row SHALL be rendered with a visually distinct highlight style (reverse video or colored background) applied to the entire row area, distinguishing it from unselected rows.

#### Scenario: Single selected row
- **WHEN** the user navigates to a row
- **THEN** that row's entire area is rendered with the highlight style and all other rows use the default style

### Requirement: Arrow key navigation
The system SHALL support Up/Down arrow keys and `j`/`k` keys to move the selection to the previous/next server entry in the list. Navigation SHALL wrap: pressing Up on the first entry does nothing, pressing Down on the last entry does nothing.

#### Scenario: Navigate down
- **WHEN** the user presses Down arrow or `k` and the selection is not on the last entry
- **THEN** the selection moves to the next entry

#### Scenario: Navigate up
- **WHEN** the user presses Up arrow or `j` and the selection is not on the first entry
- **THEN** the selection moves to the previous entry

#### Scenario: Boundary — at last entry
- **WHEN** the user presses Down arrow on the last entry
- **THEN** the selection remains on the last entry

#### Scenario: Empty list
- **WHEN** the server list is empty
- **THEN** arrow key presses have no effect

### Requirement: Key binding — new entry
The system SHALL create a new server entry when the user presses `n`. The system SHALL present an input form or dialog for the user to enter server connection details and tunnel forwards, then add the entry to the list and persist the updated configuration.

#### Scenario: Create new entry
- **WHEN** the user presses `n`
- **THEN** the app enters a creation mode/dialog for defining a new server entry

### Requirement: Key binding — edit entry
The system SHALL allow editing the selected server entry when the user presses `e`. The system SHALL present the entry's current values in an input form and persist changes on confirmation.

#### Scenario: Edit existing entry
- **WHEN** the user presses `e` with a server entry selected
- **THEN** the app enters an edit mode/dialog pre-filled with the selected entry's values

#### Scenario: Edit with empty list
- **WHEN** the user presses `e` and the list is empty
- **THEN** the action is ignored (no-op)

### Requirement: Key binding — delete entry
The system SHALL delete the selected server entry when the user presses `d`. If the entry has an active connection, it SHALL be disconnected first. The system SHALL persist the updated configuration after deletion.

#### Scenario: Delete a disconnected entry
- **WHEN** the user presses `d` with a disconnected entry selected
- **THEN** the entry is removed from the list and the config is saved

#### Scenario: Delete a connected entry
- **WHEN** the user presses `d` with a connected entry selected
- **THEN** the connection is terminated, the entry is removed, and the config is saved

#### Scenario: Delete adjusts selection
- **WHEN** the user deletes the last entry in the list
- **THEN** the selection moves to the new last entry (or the list becomes empty)

### Requirement: Key binding — toggle connect/disconnect
The system SHALL toggle the connection state of the selected server entry when the user presses `Enter`. If the entry is disconnected or failed, it SHALL initiate a connection. If the entry is connected, connecting, or reconnecting, it SHALL disconnect.

#### Scenario: Connect a disconnected entry
- **WHEN** the user presses `Enter` on a Disconnected entry
- **THEN** the system initiates a connection (state → Connecting)

#### Scenario: Disconnect a connected entry
- **WHEN** the user presses `Enter` on a Connected entry
- **THEN** the system disconnects (state → Disconnected)

### Requirement: Key binding — quit
The system SHALL exit the application when the user presses `q`. Before exiting, the system SHALL terminate all active ssh connections cleanly.

#### Scenario: Quit with active connections
- **WHEN** the user presses `q` and there are active ssh connections
- **THEN** all connections are terminated and the app exits

#### Scenario: Quit with no connections
- **WHEN** the user presses `q` and there are no active connections
- **THEN** the app exits immediately

### Requirement: Color-coded connection state
The system SHALL apply semantic colors to connection state indicators in each row: green for Connected, dim/gray for Disconnected, yellow for Connecting and Reconnecting, red for Failed. Forward type labels (`L`, `R`, `D`) SHALL be rendered in cyan. The selected row SHALL use a highlighted background (reverse or blue).

#### Scenario: Connected server row
- **WHEN** a server entry is in Connected state
- **THEN** its status indicator is rendered in green

#### Scenario: Failed server row
- **WHEN** a server entry is in Failed state
- **THEN** its status indicator is rendered in red

#### Scenario: Forward type label colors
- **WHEN** a forward line displays a Local forward
- **THEN** the "L" type label is rendered in cyan
