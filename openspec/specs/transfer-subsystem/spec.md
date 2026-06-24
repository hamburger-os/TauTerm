# transfer-subsystem

## Purpose

定义 TauTerm 的多策略传输子系统，根据会话协议自动选择 Inline（端口移交）、SideChannel（协议子通道）或 SeparateConnection（独立连接）传输策略。

## Requirements

### Requirement: Transfer Manager selects strategy based on channel capability
The system SHALL provide a TransferManager that inspects the active session's channel and protocol capabilities to select the appropriate transfer strategy.
The three strategies SHALL be: `Inline` (channel supports handoff, used for serial YModem/XModem/ZModem), `SideChannel` (protocol supports multiplexed sub-channels, used for SSH SFTP/SCP), and `SeparateConnection` (protocol requires independent data connections, used for FTP).
The Transfer Manager SHALL NOT require the caller to specify a strategy—it SHALL be derived automatically.

#### Scenario: Serial YModem transfer uses Inline strategy
- **WHEN** a YModem send is initiated on a Serial session
- **THEN** the Transfer Manager SHALL detect `channel.try_handoff()` returning `Some`, and use Inline strategy with port handoff

#### Scenario: SSH SFTP transfer uses SideChannel strategy
- **WHEN** an SFTP transfer is initiated on an SSH session
- **THEN** the Transfer Manager SHALL detect handoff returning `None`, check the SSH plugin's `transfer_protocols()` for SFTP, and open an SFTP sub-channel over the existing SSH session

#### Scenario: FTP transfer uses SeparateConnection strategy
- **WHEN** an FTP file upload is initiated
- **THEN** the Transfer Manager SHALL establish a separate data connection (PASV or PORT mode) independent of the control connection

### Requirement: Inline strategy pauses and resumes I/O loop
The Inline transfer strategy SHALL pause the I/O loop via the handoff mechanism, take exclusive ownership of the transport, execute the transfer, then return the transport to the I/O loop.
The session SHALL remain in "Connected" state throughout—the transport is never closed, only temporarily reassigned.
The session state SHALL transition to "Transferring" during Inline transfer and back to "Connected" upon completion.

#### Scenario: I/O loop resumes after Inline transfer
- **WHEN** a YModem send completes and the port is returned via `return_tx.send(port)`
- **THEN** the I/O loop SHALL receive the port, clear residual buffers, and resume normal read/write cycle

#### Scenario: Inline transfer is cancelled
- **WHEN** the user cancels an Inline YModem transfer
- **THEN** the port SHALL be returned to the I/O loop immediately and the session state SHALL return to "Connected"

### Requirement: SideChannel strategy multiplexes over existing connection
The SideChannel strategy SHALL open a protocol-specific sub-channel within the existing session without affecting the terminal I/O stream.
The terminal SHALL continue to send and receive data during SideChannel transfers.
The SideChannel strategy SHALL be used for SSH SFTP and SCP transfers.

#### Scenario: SFTP transfer while terminal is active
- **WHEN** an SFTP file download starts on an SSH session
- **THEN** the terminal SHALL continue to display output and accept input while the file transfers in parallel

#### Scenario: SideChannel sub-channel open failure
- **WHEN** the SSH server rejects the SFTP subsystem request
- **THEN** the Transfer Manager SHALL emit a transfer failure event without affecting the terminal connection

### Requirement: SeparateConnection strategy manages independent data channel
The SeparateConnection strategy SHALL establish a new network connection for data transfer while keeping the control connection active.
The control connection SHALL remain usable for commands and status during separate-connection transfers.
The data connection SHALL be closed upon transfer completion while the control connection persists.

#### Scenario: FTP passive mode data transfer
- **WHEN** an FTP file download is initiated in passive mode
- **THEN** the Transfer Manager SHALL open a data connection to the PASV address/port, transfer the file, and close the data connection while the FTP control session remains connected

### Requirement: Transfer progress events are strategy-agnostic
All three transfer strategies SHALL emit the same `transfer-progress`, `transfer-file-start`, `transfer-file-complete`, and `transfer-complete` events with identical payload schemas.
The frontend SHALL NOT need to know which transfer strategy is in use.

#### Scenario: Progress events from SSH SFTP
- **WHEN** SFTP transfer progress updates
- **THEN** the same `transfer-progress` event payload SHALL be emitted as for YModem transfers, with `direction`, `file_name`, `bytes_transferred`, and `total_bytes`

### Requirement: Transfer protocol registry is extensible
The system SHALL maintain a protocol registry mapping `(protocol_type, strategy)` tuples to transfer implementations.
Plugins SHALL register transfer protocol handlers via `TransferManager::register_protocol(plugin_id, protocol_type, strategy, handler)`.
Built-in transfer protocols SHALL include: YModem, XModem, ZModem (Inline), SFTP, SCP (SideChannel), and FTP (SeparateConnection).

#### Scenario: Plugin registers a custom transfer protocol
- **WHEN** a future plugin registers "kermit" under the Inline strategy
- **THEN** the Transfer Manager SHALL make Kermit available for any session whose channel supports handoff
