# bottom-panel-static (delta)

## Purpose

更新底部静态信息面板的会话信息显示要求，从单行居中布局迁移到左右分栏布局。

## MODIFIED Requirements

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

The two columns SHALL be separated by a glass-border divider (`1px solid var(--glass-border)`).

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
