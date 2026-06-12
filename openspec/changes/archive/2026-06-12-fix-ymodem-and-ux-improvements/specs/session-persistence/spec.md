## ADDED Requirements

### Requirement: Sessions saved on app close

The application SHALL automatically save all active session configurations to disk when the application window closes. Each saved session SHALL include its id, name, connection type, endpoint, and connection parameters.

#### Scenario: Sessions saved on window close

- **WHEN** the user closes the application window while sessions exist in the tab bar
- **THEN** a `sessions.json` file is written to the app data directory containing all session configurations

#### Scenario: No sessions saves empty file

- **WHEN** the user closes the application with no active sessions
- **THEN** an empty array or no `sessions.json` is written

### Requirement: Sessions restored on app start

The application SHALL read saved session configurations from disk on startup and restore them as disconnected tabs in the sidebar. Restored sessions SHALL NOT auto-connect; the user must manually reconnect each session.

#### Scenario: Restored tabs appear on startup

- **WHEN** the application starts and `sessions.json` contains 3 saved sessions
- **THEN** the sidebar displays 3 tabs with their saved names, all in "disconnected" state

#### Scenario: No saved file shows empty sidebar

- **WHEN** the application starts and no `sessions.json` exists
- **THEN** the sidebar shows no tabs and the terminal area is empty

#### Scenario: Corrupted session file is handled gracefully

- **WHEN** the application starts and `sessions.json` is corrupted
- **THEN** the corrupted file is backed up as `sessions.json.bak`, no tabs are restored, and the application starts normally

### Requirement: Restored session can be reconnected

The user SHALL be able to click a restored tab and use the connect dialog or serial config sidebar to reconnect to the saved endpoint with the saved parameters.

#### Scenario: Reconnect from restored tab

- **WHEN** user clicks a restored (disconnected) tab
- **THEN** the serial config sidebar populates with the saved parameters (endpoint, baud rate, etc.)
- **AND** the user can click "Connect" to reconnect with those parameters
