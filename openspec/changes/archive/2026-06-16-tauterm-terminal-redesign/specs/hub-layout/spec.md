# Hub Layout

## ADDED Requirements

### Requirement: App Shell Layout
The system SHALL present a Hub-style layout consisting of:
1. Quick Connect Bar (top)
2. Session Sidebar (left)
3. Tab Bar + Terminal Area (center)
4. File Transfer Panel (bottom, collapsible)
5. Status Bar (bottom)

#### Scenario: Default layout
- **WHEN** application starts with no active sessions
- **THEN** all layout zones are visible, terminal area shows empty/placeholder state

### Requirement: Quick Connect Bar
The system SHALL display an always-visible quick connect bar at the top.
The system SHALL accept a connection string in the format `<protocol>://<endpoint>` or a plain endpoint name.
The system SHALL display a "Connect" button that creates a new session in a new tab.

#### Scenario: Quick connect via serial
- **WHEN** user types "COM3" in the quick connect bar and presses Enter
- **THEN** a new serial session is created with default parameters (115200-8N1) in a new tab

#### Scenario: Quick connect with params
- **WHEN** user types "COM3:921600" in the quick connect bar
- **THEN** a new serial session is created with baud rate 921600

### Requirement: Session Sidebar
The system SHALL display a session list sidebar on the left side.
The sidebar SHALL list all saved and active sessions with status indicators.
The sidebar SHALL include a search bar to filter sessions.
The sidebar SHALL include an "Add Session" button.

#### Scenario: Session list display
- **WHEN** 3 sessions are saved and 1 is active
- **THEN** all 3 are displayed, the active one shows a green glow indicator

#### Scenario: Session search
- **WHEN** user types "router" in the session search bar
- **THEN** only sessions whose name or endpoint contains "router" are displayed

### Requirement: Resizable Sidebar
The system SHALL allow resizing the session sidebar width between 180px and 400px.
The resize handle SHALL glow on hover.

#### Scenario: Resize sidebar
- **WHEN** user drags the sidebar resize handle
- **THEN** sidebar width changes smoothly with a glowing cyan indicator line

### Requirement: Collapsible Sidebar
The system SHALL allow collapsing the session sidebar to 0px via toggle.
The system SHALL show a thin expand bar when collapsed.

#### Scenario: Toggle sidebar
- **WHEN** user presses the sidebar toggle button
- **THEN** sidebar collapses with slide animation, terminal area expands to fill space

### Requirement: Status Bar
The system SHALL display a status bar showing:
- Connection status indicator (dot + label)
- Rx/Tx byte counters for the active session
- Current session info (protocol, endpoint, parameters)

#### Scenario: Connected status display
- **WHEN** a serial session is active on COM3 at 115200
- **THEN** status bar shows green dot, "COM3 115200-8N1", and live Rx/Tx counters
