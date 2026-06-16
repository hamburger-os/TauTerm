# session-manager (delta)

## MODIFIED Requirements

### Requirement: Create Session
The system SHALL allow creating a new terminal session by providing a connection type, endpoint, and parameters.
The system SHALL assign each session a unique identifier.
The system SHALL return the session ID and initial state.
The system SHALL store a cancel-transfer channel in the session handle for future cancellation requests.
Session implementation SHALL use a concrete `SessionImpl` enum (Serial, with Ssh/Telnet reserved) rather than a `TermSession` trait with stub methods.

#### Scenario: Create serial session
- **WHEN** user connects to COM3 with baud_rate=115200
- **THEN** system creates a new `SessionImpl::Serial`, starts an I/O thread, returns session ID and "connected" state. The session handle contains an `Option<Sender<()>>` field initialized to `None` (set when a transfer starts).

#### Scenario: Create session with invalid parameters
- **WHEN** user attempts to connect with nonexistent port "COM99"
- **THEN** system returns a typed `TauTermError::SerialPortNotFound` error and does not create a session

### Requirement: Cancel Transfer
The system SHALL allow cancelling an in-progress file transfer for a specific session.
The system SHALL use the stored cancel channel in `SessionHandle` to signal cancellation.

#### Scenario: Cancel active transfer
- **WHEN** user clicks "Cancel Transfer" during an active YModem transfer
- **THEN** system sends signal via `SessionHandle.cancel_transfer_tx`, transfer thread receives and aborts with CAN sequence. After cancellation, the channel sender is set to `None`.

#### Scenario: Cancel when no transfer active
- **WHEN** `cancel_transfer` is called for a session with no active transfer
- **THEN** system silently returns (no-op), no error thrown
