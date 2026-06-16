# session-configure

## Purpose

定义会话配置对话框要求，包括上下文菜单 Configure 选项和重连参数预填。

## Requirements

### Requirement: Context menu provides Configure Session for all sessions

The session context menu SHALL provide a "Configure" option for both connected and disconnected sessions, and SHALL NOT provide a separate "Rename" option.

#### Scenario: Connected session context menu

- **WHEN** user right-clicks a connected session in the sidebar
- **THEN** the context menu shows "Disconnect", "Configure", and "Delete" options, and does NOT show a "Rename" option

#### Scenario: Disconnected session context menu

- **WHEN** user right-clicks a disconnected session in the sidebar
- **THEN** the context menu shows "Connect", "Configure", and "Delete" options, and does NOT show a "Rename" option

### Requirement: Configure opens ConnectDialog with session parameters pre-filled

The system SHALL open the ConnectDialog with all session parameters pre-filled when the "Configure" option is selected from the context menu, regardless of whether the session is connected or disconnected.

#### Scenario: Configure a disconnected session

- **WHEN** user selects "Configure" on a disconnected session
- **THEN** the ConnectDialog opens to the configuration step with port, baud rate, data bits, parity, stop bits, flow control, and session name pre-filled from the session's saved parameters

#### Scenario: Configure a connected session

- **WHEN** user selects "Configure" on a connected session
- **THEN** the ConnectDialog opens to the configuration step with port, baud rate, data bits, parity, stop bits, flow control, and session name pre-filled from the active session's parameters

### Requirement: Reconnect flow disconnects before reconnecting

When configuring a connected session, the system SHALL first disconnect the existing connection before establishing a new connection with the updated parameters, ensuring the serial port is released before reopening.

#### Scenario: Reconnect with new parameters

- **WHEN** user changes parameters (e.g., baud rate) in the Configure dialog for a connected session and clicks the connect button
- **THEN** the system disconnects the existing session, then opens a new connection with the updated parameters using the same serial port
