# protocol-plugins

## Purpose

定义 TauTerm 的内建协议插件集，每个协议作为独立插件实现 `ProtocolAdapter` trait 并提供前端 UI 组件。

## ADDED Requirements

### Requirement: Serial plugin provides serial port terminal sessions
The system SHALL include a built-in `serial` plugin implementing `ProtocolAdapter` for RS-232/RS-485 serial port communication.
The Serial plugin SHALL support baud rate, data bits, parity, stop bits, flow control, and data mode (text/hex) configuration.
The Serial plugin SHALL enumerate available serial ports via `discover_endpoints()`.
The Serial plugin SHALL declare capabilities: `connection`, `transfer`, `endpoint_discovery`.
The Serial plugin SHALL support YModem, XModem, and ZModem transfer protocols via Inline strategy.

#### Scenario: Serial port enumeration
- **WHEN** the Serial plugin's `discover_endpoints()` is called on Windows
- **THEN** it SHALL return all available COM ports with descriptive names

#### Scenario: Serial HEX mode display
- **WHEN** the serial session is configured with `data_mode: "hex"`
- **THEN** incoming data SHALL be formatted as hex dump lines (offset + hex columns + ASCII) before rendering in the terminal

### Requirement: SSH plugin provides secure shell sessions
The system SHALL include a built-in `ssh` plugin implementing `ProtocolAdapter` for SSH connections.
The SSH plugin SHALL support password authentication, SSH key authentication, and SSH agent authentication.
The SSH plugin SHALL support host key verification via known_hosts.
The SSH plugin SHALL declare capabilities: `connection`, `transfer`, `authentication`, `credential_store`, `network_outbound`.
The SSH plugin SHALL support SFTP and SCP transfer protocols via SideChannel strategy.

#### Scenario: SSH connection with key authentication
- **WHEN** the user connects to an SSH host with a private key from the Credential Store
- **THEN** the SSH plugin SHALL retrieve the key, authenticate, and establish an interactive shell session

#### Scenario: SSH SFTP file transfer
- **WHEN** the user initiates an SFTP download on an active SSH session
- **THEN** the SSH plugin SHALL open an SFTP sub-channel and transfer files without disrupting the terminal session

### Requirement: Telnet plugin provides telnet protocol sessions
The system SHALL include a built-in `telnet` plugin implementing `ProtocolAdapter` for Telnet connections (RFC 854).
The Telnet plugin SHALL support basic Telnet option negotiation (echo, terminal type, window size).
The Telnet plugin SHALL declare capabilities: `connection`, `network_outbound`.

#### Scenario: Telnet connection with option negotiation
- **WHEN** the user connects to a Telnet host
- **THEN** the plugin SHALL negotiate basic Telnet options (WILL/WONT/DO/DONT) and establish a terminal session

### Requirement: TCP Raw plugin provides raw TCP socket sessions
The system SHALL include a built-in `tcp-raw` plugin implementing `ProtocolAdapter` for raw TCP socket connections.
The TCP Raw plugin SHALL support configurable host, port, and connection timeout.
The TCP Raw plugin SHALL declare capabilities: `connection`, `network_outbound`.
The TCP Raw plugin SHALL NOT perform any protocol negotiation—data is passed directly between the socket and the terminal.

#### Scenario: Raw TCP connection for debugging
- **WHEN** the user connects to `example.com:8080` via TCP Raw
- **THEN** the plugin SHALL open a TCP socket and relay raw bytes bidirectionally between the terminal and the socket

### Requirement: TRDP plugin provides TRDP protocol sessions
The system SHALL include a built-in `trdp` plugin implementing `ProtocolAdapter` for TRDP (Train Real-time Data Protocol) connections.
The TRDP plugin SHALL support TRDP message framing and optional TLS encryption.
The TRDP plugin SHALL declare capabilities: `connection`, `authentication`, `credential_store`, `network_outbound`.

#### Scenario: TRDP connection with TLS
- **WHEN** the user connects to a TRDP endpoint with TLS enabled
- **THEN** the plugin SHALL perform TLS handshake with certificate validation before exchanging TRDP messages

### Requirement: Shell Local plugin provides local terminal sessions
The system SHALL include a built-in `shell-local` plugin implementing `ProtocolAdapter` for local shell (PTY) sessions.
The Shell Local plugin SHALL spawn the user's default shell (cmd/powershell on Windows, bash/zsh on Unix).
The Shell Local plugin SHALL support terminal size synchronization with the PTY.
The Shell Local plugin SHALL declare capabilities: `connection`, `stream`.

#### Scenario: Local PowerShell session on Windows
- **WHEN** the user creates a Shell Local session
- **THEN** the plugin SHALL spawn a PowerShell process connected to a PTY and relay I/O to the terminal

### Requirement: FTP plugin provides FTP client sessions
The system SHALL include a built-in `ftp` plugin implementing `ProtocolAdapter` for FTP connections.
The FTP plugin SHALL support active and passive mode, anonymous and authenticated login.
The FTP plugin SHALL use `content_type: "file_browser"` for its tab content.
The FTP plugin SHALL declare capabilities: `connection`, `transfer`, `authentication`, `credential_store`, `network_outbound`, `filesystem_access`.
The FTP plugin SHALL support file transfer via SeparateConnection strategy.

#### Scenario: FTP passive mode connection
- **WHEN** the user connects to an FTP server in passive mode
- **THEN** the plugin SHALL send PASV, establish a data connection to the returned address/port, and render the remote file tree in the file browser view

### Requirement: iPerf3 plugin provides network performance testing
The system SHALL include a built-in `iperf3` plugin implementing `ProtocolAdapter` for iPerf3 network performance testing.
The iPerf3 plugin SHALL use `content_type: "stats_dashboard"` for its tab content.
The iPerf3 plugin SHALL support client mode (outbound test) and server mode (listen for incoming tests).
The iPerf3 plugin SHALL display real-time throughput charts, jitter, and packet loss statistics.
The iPerf3 plugin SHALL declare capabilities: `network_outbound`, `network_listen`.

#### Scenario: iPerf3 client throughput test
- **WHEN** the user runs an iPerf3 client test against a remote server
- **THEN** the stats dashboard SHALL display real-time throughput (Mbps), with charts updating at 1-second intervals
