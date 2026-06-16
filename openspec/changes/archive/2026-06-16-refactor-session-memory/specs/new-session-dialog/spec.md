## ADDED Requirements

### Requirement: New session dialog has mode-first design
The new session dialog SHALL present a two-step flow: first select a connection mode, then configure mode-specific parameters. The mode selection SHALL be presented as a grid of visually distinct mode cards.

#### Scenario: User opens the new session dialog
- **WHEN** the user clicks the "New Session" toolbar button or triggers the new-session keyboard shortcut
- **THEN** a modal dialog opens showing a grid of connection mode cards (Serial, SSH, Telnet, TFTP)
- **AND** each card SHALL display an icon, name, and brief description

#### Scenario: User selects the Serial mode card
- **WHEN** the user clicks the "Serial" mode card
- **THEN** the dialog SHALL transition to show the serial configuration form
- **AND** the form SHALL include: port selector, baud rate, data bits, parity, stop bits, flow control, and optional session name

#### Scenario: User selects an unavailable mode card
- **WHEN** the user clicks an SSH, Telnet, or TFTP mode card
- **THEN** the dialog SHALL show that mode's configuration form with placeholder fields
- **AND** a "Coming Soon" badge SHALL be visible
- **AND** the Connect button SHALL be disabled

### Requirement: Mode cards support extensibility
The dialog SHALL be structured so adding a new connection mode requires only a new card component and a new config panel component.

#### Scenario: Developer adds a new connection mode
- **WHEN** a developer adds a new mode definition and corresponding card + config panel components
- **THEN** the new mode SHALL appear in the mode selection grid without modifying the dialog's core layout logic
- **AND** the config panel SHALL render when that mode is selected

### Requirement: Serial mode configuration is fully functional
The Serial configuration panel SHALL provide all fields to establish a serial connection and SHALL connect to the selected port with the specified parameters.

#### Scenario: User configures and connects via serial
- **WHEN** the user selects a serial port, sets parameters (baud rate, data bits, parity, stop bits, flow control), and clicks Connect
- **THEN** the system SHALL invoke `connect_session` with the serial port endpoint and parameters
- **AND** upon successful connection, the dialog SHALL close
- **AND** a new session entry SHALL appear in the sidebar in "connecting" then "connected" state

#### Scenario: User configures but no port is available
- **WHEN** no serial ports are detected
- **THEN** the port selector SHALL show "No serial ports detected"
- **AND** the Connect button SHALL be disabled

### Requirement: Session name is optional with smart default
The session name field SHALL be optional. When left empty, the system SHALL generate a default name from the connection type and endpoint.

#### Scenario: User leaves session name empty
- **WHEN** the user connects without entering a session name
- **THEN** the session SHALL be named with the format "<ConnectionType> @ <Endpoint>" (e.g., "Serial @ COM3")
