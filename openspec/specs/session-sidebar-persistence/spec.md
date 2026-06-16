# session-sidebar-persistence

## Purpose

定义会话侧栏持久化要求，包括断开会话保留、无数量限制和无标签栏终端区域。

## Requirements

### Requirement: Disconnected sessions persist in sidebar
The system SHALL retain session entries in the sidebar after the underlying connection is terminated. A disconnected session SHALL remain visible with a gray status indicator and its saved connection parameters intact.

#### Scenario: User disconnects a connected session
- **WHEN** a connected session is disconnected (via backend event or user action)
- **THEN** the session entry remains in the sidebar with its status dot turned gray
- **AND** the session's connection parameters (endpoint, baud rate, etc.) are preserved

#### Scenario: Backend emits session-disconnected event
- **WHEN** the Rust backend emits a `session-disconnected` event for an active session
- **THEN** the frontend dispatches SET_TAB_STATE with state "disconnected" instead of REMOVE_TAB
- **AND** the session remains in the sidebar list

### Requirement: No session count limit
The system SHALL NOT impose a hard limit on the number of session entries in the sidebar. The sidebar list SHALL be scrollable and searchable to handle any number of entries.

#### Scenario: User has many historical sessions
- **WHEN** the sidebar contains more sessions than can fit in the visible area
- **THEN** the list SHALL be vertically scrollable
- **AND** no counter or limit indicator (e.g., "N/10") SHALL be displayed

#### Scenario: User searches through many sessions
- **WHEN** the user types in the sidebar search field
- **THEN** the list SHALL filter to matching sessions regardless of total count
- **AND** search performance SHALL not degrade with many entries

### Requirement: Terminal area has no session switcher
The terminal viewport SHALL NOT contain a tab bar or any session-switching UI. Session switching SHALL be handled exclusively through the sidebar.

#### Scenario: User views the terminal area
- **WHEN** the app is rendered with one or more sessions
- **THEN** no tab bar is visible above the terminal
- **AND** only the active session's terminal is displayed

#### Scenario: User switches session via sidebar
- **WHEN** the user clicks a different session in the sidebar
- **THEN** the terminal area SHALL animate to the newly selected session's terminal view
