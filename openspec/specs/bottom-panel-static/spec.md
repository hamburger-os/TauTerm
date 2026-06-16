# bottom-panel-static

## Purpose

定义底部静态信息面板要求，替代原有的可切换文件传输面板。

## Requirements

### Requirement: Bottom panel is always visible at fixed height
The bottom panel SHALL be a static, always-visible component at a fixed default height. It SHALL NOT be toggleable or resizable via toolbar actions.

#### Scenario: App renders with sessions
- **WHEN** the app is loaded
- **THEN** a bottom info panel SHALL be visible at a fixed height (200px, matching the former PANEL_DEFAULT)
- **AND** no toggle mechanism exists to show/hide this panel

### Requirement: File transfer panel and its controls are removed
The file transfer panel, its resize handle, and all toggle UI SHALL be removed from the application.

#### Scenario: User looks for file transfer functionality
- **WHEN** the user inspects the toolbar and app layout
- **THEN** no file transfer button or panel SHALL be visible
- **AND** the bottom panel SHALL display static info content instead of transfer controls

### Requirement: Bottom panel displays session info
The bottom panel SHALL display relevant information about the current active session or app state.

#### Scenario: User has an active serial session
- **WHEN** a serial session is active
- **THEN** the bottom panel SHALL display the session name, connection type, endpoint, and connection status

#### Scenario: No active session exists
- **WHEN** no session is active
- **THEN** the bottom panel SHALL display a placeholder message (e.g., "No active session" or app info)
