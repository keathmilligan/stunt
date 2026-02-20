## ADDED Requirements

### Requirement: Server entry data model
The system SHALL represent each SSH server as a `ServerEntry` struct containing: a unique name (string), host (string), port (u16, default 22), user (optional string), and identity_file (optional path). Each `ServerEntry` SHALL contain a vector of `TunnelForward` definitions.

#### Scenario: Minimal server entry
- **WHEN** a server entry is defined with only a name and host
- **THEN** port defaults to 22, user and identity_file are None, and forwards is an empty vector

#### Scenario: Fully specified server entry
- **WHEN** a server entry is defined with name, host, port, user, identity_file, and one or more forwards
- **THEN** all fields are stored and accessible on the struct

### Requirement: Tunnel forward type model
The system SHALL represent tunnel forwards as a `TunnelForward` enum with three variants: `Local` (bind_address, bind_port, remote_host, remote_port), `Remote` (bind_address, bind_port, remote_host, remote_port), and `Dynamic` (bind_address, bind_port). Each variant SHALL carry only the addressing fields relevant to its forward type.

#### Scenario: Local forward
- **WHEN** a `TunnelForward::Local` is created with bind_port 5432, remote_host "db.internal", remote_port 5432
- **THEN** it maps to the ssh flag `-L 5432:db.internal:5432`

#### Scenario: Remote forward
- **WHEN** a `TunnelForward::Remote` is created with bind_port 8080, remote_host "localhost", remote_port 3000
- **THEN** it maps to the ssh flag `-R 8080:localhost:3000`

#### Scenario: Dynamic SOCKS forward
- **WHEN** a `TunnelForward::Dynamic` is created with bind_port 1080
- **THEN** it maps to the ssh flag `-D 1080`

### Requirement: TOML serialization and deserialization
The system SHALL serialize and deserialize the complete list of `ServerEntry` values to and from TOML format using serde. The TOML schema SHALL use `[[server]]` as the top-level array of tables, with nested `[[server.forwards]]` for tunnel definitions. The forward type SHALL be stored as a `type` field with values `"local"`, `"remote"`, or `"dynamic"`.

#### Scenario: Round-trip serialization
- **WHEN** a list of server entries is serialized to TOML and then deserialized back
- **THEN** the resulting data is identical to the original

#### Scenario: Deserialize with missing optional fields
- **WHEN** a TOML file omits the `user` and `identity_file` fields for a server entry
- **THEN** those fields deserialize as None without error

#### Scenario: Invalid TOML input
- **WHEN** the TOML content is malformed or contains unknown fields
- **THEN** deserialization returns a descriptive error

### Requirement: Platform-appropriate persistent storage
The system SHALL store tunnel configuration in a file named `tunnels.toml` inside a `tunnel-mgr` subdirectory of the platform user data directory (as resolved by the `dirs` crate's `data_dir()`). The system SHALL create the directory and file if they do not exist on first run.

#### Scenario: First run with no existing config
- **WHEN** the app starts and no `tunnels.toml` exists
- **THEN** the system creates the directory and an empty config file, and the app starts with an empty server list

#### Scenario: Existing config loaded on startup
- **WHEN** the app starts and a valid `tunnels.toml` exists
- **THEN** the system loads all server entries from the file

#### Scenario: Data directory resolution per platform
- **WHEN** running on Linux
- **THEN** the config path is `~/.local/share/tunnel-mgr/tunnels.toml`

### Requirement: Atomic config file writes
The system SHALL write configuration changes atomically by writing to a temporary file in the same directory and then renaming it to `tunnels.toml`. The system SHALL keep a `.bak` copy of the previous file before overwriting.

#### Scenario: Save with existing config
- **WHEN** the user saves changes and a `tunnels.toml` already exists
- **THEN** the existing file is renamed to `tunnels.toml.bak`, the new content is written to a tempfile, and the tempfile is renamed to `tunnels.toml`

#### Scenario: Save with write failure
- **WHEN** the tempfile write fails (e.g., disk full)
- **THEN** the original `tunnels.toml` remains unchanged and an error is returned
