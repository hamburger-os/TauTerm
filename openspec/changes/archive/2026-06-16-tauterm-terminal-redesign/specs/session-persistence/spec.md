# Session Persistence

## ADDED Requirements

### Requirement: Save Sessions on Change
The system SHALL automatically save all session configurations to a JSON file whenever a session is created, modified, or closed.
The file SHALL be stored at the platform-appropriate config directory.

#### Scenario: Auto-save after creating a session
- **WHEN** user creates a new serial session on COM3
- **THEN** the session configuration is automatically written to the sessions JSON file

#### Scenario: Auto-save after renaming
- **WHEN** user renames a session tab
- **THEN** the new name is persisted to the JSON file

### Requirement: Restore Sessions on Startup
The system SHALL read the sessions JSON file on application startup.
The system SHALL restore saved sessions as tab placeholders (disconnected state).
The system SHALL NOT auto-connect sessions on startup.

#### Scenario: Startup with saved sessions
- **WHEN** application starts and 3 sessions were saved from the previous run
- **THEN** 3 tab placeholders are created with their saved names and connection parameters

#### Scenario: First startup
- **WHEN** application starts with no saved sessions file
- **THEN** the application shows empty state with no tabs

### Requirement: Session Data Format
Each saved session SHALL contain: id, name, connection_type, endpoint, params, timestamp.
The file format SHALL be human-readable JSON.

#### Scenario: Session JSON structure
- **WHEN** a serial session named "Router Debug" on COM3 at 115200 is saved
- **THEN** the JSON entry contains all fields including type "serial", endpoint "COM3", params with baud_rate 115200

### Requirement: Graceful File Corruption Handling
The system SHALL catch JSON parse errors when loading sessions.
The system SHALL fall back to an empty session list on parse failure.
The system SHALL backup the corrupted file as `.bak`.

#### Scenario: Corrupted sessions file
- **WHEN** the sessions.json file is corrupted (invalid JSON)
- **THEN** system logs a warning, creates a .bak copy, and starts with an empty session list
