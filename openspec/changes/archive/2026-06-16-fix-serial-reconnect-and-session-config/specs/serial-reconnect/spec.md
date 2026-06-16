## ADDED Requirements

### Requirement: Serial port reopens reliably after disconnect

The system SHALL ensure that after a session is disconnected, the same serial port can be reopened successfully within a subsequent connect/reconnect operation, without requiring session deletion.

#### Scenario: Disconnect then reconnect succeeds

- **WHEN** user disconnects a connected serial session and then reconnects to the same COM port via the Connect dialog
- **THEN** the serial port opens successfully and data can be sent and received through the port

#### Scenario: Rapid disconnect-reconnect cycle

- **WHEN** user disconnects and immediately reconnects to the same COM port within 1 second
- **THEN** the system retries port opening up to 3 times with a 100ms delay between attempts, and succeeds if the port becomes available within the retry window

### Requirement: Backend session state is cleaned up on disconnect

The system SHALL properly remove all backend resources (I/O thread, port handle, channels) associated with a session when it is disconnected, so that no stale session handles interfere with subsequent connections.

#### Scenario: Explicit disconnect cleans up backend

- **WHEN** user explicitly disconnects a session via the context menu
- **THEN** the backend removes the session handle from the session map, stops and joins the I/O thread, and releases the serial port

#### Scenario: Unexpected disconnect cleans up backend

- **WHEN** a serial port experiences an I/O error (e.g., device unplugged)
- **THEN** the backend marks the session as disconnected and clears I/O thread references so the stale handle does not accumulate

#### Scenario: YModem transfer completion cleans up backend

- **WHEN** a YModem file transfer completes and disconnects the session I/O thread
- **THEN** the backend marks the session as disconnected so reconnection can proceed cleanly
