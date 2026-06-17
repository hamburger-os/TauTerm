# toolbar-simplification (delta)

## MODIFIED Requirements

### Requirement: Toolbar contains only essential actions
The toolbar SHALL display exactly four global action buttons distributed across a three-zone layout: New Session and Sidebar Toggle on the left, Command Palette and Settings on the right. Plugin toolbar items SHALL be dynamically injected into the appropriate zone based on their `position` declaration.

#### Scenario: User views the toolbar
- **WHEN** the app is rendered
- **THEN** the toolbar SHALL display a left zone containing the logo "⚡ TauTerm", a ➕ New Session button, and a ☰ Sidebar button
- **AND** the toolbar SHALL display a right zone containing a ⌘ Commands button and a ⚙ Settings button
- **AND** the toolbar SHALL display a center zone reserved for plugin-injected items
- **AND** no "Refresh" or "Transfer" global button SHALL be visible (plugin buttons are separately injected)

#### Scenario: Plugin injects left-zone toolbar items
- **WHEN** a session using the Serial plugin is active
- **THEN** the Serial plugin's toolbar items with `position: "left"` SHALL appear in the left zone
- **AND** items with `position: "right"` SHALL appear in the right zone

#### Scenario: No active plugin session
- **WHEN** no session is active
- **THEN** the plugin toolbar zones (left plugin, center, right plugin) SHALL be empty
- **AND** the four global buttons SHALL remain visible and functional

### Requirement: Settings button placeholder
The Settings button SHALL trigger a placeholder response since the full settings dialog is out of scope for this change.

#### Scenario: User clicks the Settings button
- **WHEN** the user clicks the "Settings" button in the toolbar
- **THEN** a toast notification SHALL appear indicating "Settings coming soon"
- **OR** an empty settings panel/modal SHALL open with a title

### Requirement: Removed actions are still available where needed
Actions removed from the toolbar SHALL remain accessible through other UI elements where they are relevant.

#### Scenario: User needs to refresh serial ports
- **WHEN** the user opens the ConnectDialog in Serial mode
- **THEN** a refresh button SHALL be available next to the port selector to enumerate available ports
