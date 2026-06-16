## ADDED Requirements

### Requirement: Toolbar contains only essential actions
The toolbar SHALL display exactly four action buttons: New Session, Sidebar Toggle, Command Palette, and Settings. All other buttons SHALL be removed.

#### Scenario: User views the toolbar
- **WHEN** the app is rendered
- **THEN** the toolbar SHALL display four buttons with icons and labels: ➕ New Session, ☰ Sidebar, ⌘ Commands, ⚙ Settings
- **AND** no "Refresh" or "Transfer" button SHALL be visible

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
