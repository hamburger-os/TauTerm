## ADDED Requirements

### Requirement: Right-click opens context menu on session item
The system SHALL display a context menu when the user right-clicks a session entry in the sidebar. The menu SHALL appear at the cursor position and SHALL NOT be clipped by the sidebar or window boundaries.

#### Scenario: User right-clicks a connected session
- **WHEN** the user right-clicks a session entry whose state is "connected"
- **THEN** a context menu appears with actions: Disconnect, Rename, Delete
- **AND** the menu is positioned at the cursor location

#### Scenario: User right-clicks a disconnected session
- **WHEN** the user right-clicks a session entry whose state is "disconnected"
- **THEN** a context menu appears with actions: Connect, Configure, Rename, Delete
- **AND** the menu is positioned at the cursor location

#### Scenario: User clicks outside the context menu
- **WHEN** the context menu is open and the user clicks anywhere outside it
- **THEN** the context menu SHALL close

### Requirement: Context menu actions execute correctly
Each context menu action SHALL trigger the appropriate session operation.

#### Scenario: User selects "Connect" on a disconnected session
- **WHEN** the user clicks "Connect" in the context menu of a disconnected session
- **THEN** the system SHALL open the Configure dialog pre-filled with that session's saved parameters
- **AND** after configuring, the session SHALL attempt to connect

#### Scenario: User selects "Disconnect" on a connected session
- **WHEN** the user clicks "Disconnect" in the context menu of a connected session
- **THEN** the system SHALL close the backend connection
- **AND** the session entry SHALL remain in the sidebar marked as "disconnected"

#### Scenario: User selects "Configure" on a disconnected session
- **WHEN** the user clicks "Configure" in the context menu of a disconnected session
- **THEN** the ConnectDialog SHALL open with the session's saved parameters pre-filled
- **AND** the mode SHALL be set to the session's connection type

#### Scenario: User selects "Rename" on a session
- **WHEN** the user clicks "Rename" in the context menu
- **THEN** an inline edit field or rename prompt SHALL appear
- **AND** upon confirming the new name, the session entry SHALL update

#### Scenario: User selects "Delete" on a session
- **WHEN** the user clicks "Delete" in the context menu
- **THEN** the system SHALL prompt for confirmation
- **AND** upon confirmation, the session SHALL be permanently removed from the sidebar
- **AND** if the deleted session was the active session, the next available session SHALL become active

### Requirement: Context menu matches application design
The context menu SHALL use the same glassmorphism visual style as other UI elements.

#### Scenario: Context menu is rendered
- **WHEN** the context menu is displayed
- **THEN** it SHALL use the application's CSS custom properties for background, border, and text colors
- **AND** it SHALL have a backdrop blur effect consistent with other overlay components
