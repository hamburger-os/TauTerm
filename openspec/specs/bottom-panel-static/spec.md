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
The bottom panel SHALL display relevant information about the current active session or app state in a left-right split layout.

The left column (Identity, ~36% width) SHALL display:
- Session name with icon
- Connection type with icon
- Endpoint with icon
- Connection status with icon and state-dependent color treatment

The right column (Technical Details, ~64% width) SHALL display:
- Upper sub-area: Protocol-specific parameters (resolved via `ProfileResolver` per connection type)
- Lower sub-area: Real-time runtime statistics (TX bytes, RX bytes, connection uptime)

The two columns SHALL be separated by a glass-border divider (`1px solid var(--glass-border-default)`).

The panel SHALL maintain a minimum height of 120px to ensure all content areas remain visible.

#### Scenario: User has an active serial session
- **WHEN** a serial session is active with name "STM32-Debug", endpoint "COM3", state "connected", and serial parameters baud_rate=115200, data_bits=8, parity=none, stop_bits=1, flow_control=none
- **THEN** the left column SHALL display session name "STM32-Debug", connection type "Serial", endpoint "COM3", and status "Connected" with green indicator
- **AND** the right column SHALL display serial parameters (baud rate, data bits, parity, stop bits, flow control) and runtime statistics (TX bytes, RX bytes, uptime)

#### Scenario: User has an active SSH session (future)
- **WHEN** an SSH session is active
- **THEN** the right column SHALL display SSH-specific parameters (authentication method, cipher, host fingerprint) via the SSH profile resolver
- **AND** the left column SHALL display the same identity fields (name, type, host, status)

#### Scenario: No active session exists
- **WHEN** no session is active
- **THEN** the bottom panel SHALL display a centered placeholder message with icon and text
