# session-manager (Delta)

## Purpose

修改会话管理器要求——`SessionManager` 拆分为 `TabHost`（内核服务，管理标签页生命周期）和协议插件的 `ProtocolAdapter` 实现。`SessionImpl` 枚举移除，改为 trait 多态。

## MODIFIED Requirements

### Requirement: Create Session
The system SHALL allow creating a new session by providing a `plugin_id` and connection configuration.
The system SHALL delegate session creation to the Plugin Host, which looks up the plugin's `ProtocolAdapter` and calls `connect()`.
The system SHALL assign each session a unique identifier.
The system SHALL return the session ID and initial state.
The system SHALL store a cancel-transfer channel in the session handle for future cancellation requests.
Session implementation SHALL use the `ProtocolAdapter` trait with dynamic dispatch, rather than a concrete `SessionImpl` enum.

#### Scenario: Create serial session via plugin
- **WHEN** user connects with `plugin_id = "serial"`, endpoint = "COM3", params = {baud_rate: 115200}
- **THEN** the Plugin Host looks up the "serial" plugin's `ProtocolAdapter`, calls `connect("COM3", params)`, which returns a `Box<dyn Channel>`. The kernel starts an I/O loop for the channel, returns session ID and "connected" state.

#### Scenario: Create SSH session via plugin
- **WHEN** user connects with `plugin_id = "ssh"`, endpoint = "192.168.1.1:22", params = {auth: "password", username: "root"}
- **THEN** the Plugin Host looks up the "ssh" plugin, calls `connect()`, the SSH plugin opens a TCP connection and performs key exchange + authentication, returning an `SshChannel`. The kernel starts an async I/O loop, returns session ID.

#### Scenario: Create session with unknown plugin_id
- **WHEN** user attempts to connect with `plugin_id = "nonexistent"`
- **THEN** the Plugin Host returns a `SessionError::PluginNotFound` error and does not create a session

#### Scenario: Create session with invalid parameters
- **WHEN** user attempts to connect with nonexistent port "COM99"
- **THEN** the Serial plugin's `connect()` returns `Err(SessionError::ConnectionFailed { reason: "Port COM99 not found" })` and does not create a session

### Requirement: Close Session
The system SHALL allow closing any session by its ID.
The system SHALL stop the I/O loop, call the plugin's `disconnect()` method, and release all resources.
The system SHALL emit a disconnection event.

#### Scenario: Close active session
- **WHEN** user closes the currently active tab
- **THEN** system stops the session's I/O loop, calls `plugin.disconnect()`, releases the channel, and switches to the next available tab

#### Scenario: Close last session
- **WHEN** user closes the only remaining session
- **THEN** system shows the empty state (no active tab)

### Requirement: Independent I/O Per Session
The system SHALL run each session's I/O on an independent thread or tokio task, depending on the plugin's declared `IoStrategy`.
The system SHALL NOT allow one session's I/O failure to affect other sessions.

#### Scenario: One session disconnects
- **WHEN** the serial device for session A is physically unplugged
- **THEN** session A shows "disconnected" state, but session B (SSH) and session C (FTP) continue operating normally

#### Scenario: Async I/O session coexists with sync I/O session
- **WHEN** an SSH session (async tokio task) and a Serial session (sync std::thread) are both connected
- **THEN** both SHALL operate independently, each using its respective I/O strategy

## REMOVED Requirements

### Requirement: Session implementation uses SessionImpl enum
**Reason**: The `SessionImpl` enum with hard-coded variants (Serial, with Ssh/Telnet reserved) is replaced by the `ProtocolAdapter` trait with dynamic dispatch via Plugin Host. Adding a new protocol no longer requires modifying the enum or its match arms.
**Migration**: All code referencing `SessionImpl::Serial(session)` SHALL be updated to use `plugin_host.get_adapter(plugin_id)` and work with `Box<dyn ProtocolAdapter>`. The `SessionHandle.session` field type changes from `SessionImpl` to `Box<dyn ProtocolAdapter>`.
