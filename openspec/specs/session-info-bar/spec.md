# session-info-bar

## Purpose

定义重新设计的会话信息栏要求，包括左右分栏布局、协议 Profile 系统和运行时统计显示。

## Requirements

### Requirement: Left-right split layout
The session info panel SHALL render content in a two-column layout: a left identity column and a right details column, separated by a glass-border divider line.

#### Scenario: Panel renders with active serial session
- **WHEN** a serial session is active with name "STM32-Debug", endpoint "COM3", state "connected"
- **THEN** the left column SHALL display session name, connection type, endpoint, and connection status as icon-prefixed rows
- **AND** the right column SHALL display serial parameters in the top sub-area and runtime statistics in the bottom sub-area

#### Scenario: Panel renders with no active session
- **WHEN** no session is active
- **THEN** the panel SHALL display a centered empty state message with icon and text

#### Scenario: Panel minimum height constraint
- **WHEN** the bottom panel is resized below 120px
- **THEN** the info panel content SHALL remain fully visible (no clipping) via the panel's min-height constraint

### Requirement: Protocol-agnostic profile system
The info panel SHALL use a resolver function pattern to render protocol-specific content. Each connection type SHALL provide a `ProfileResolver` function that returns a `SessionProfile` data structure containing `identity` and `parameters` arrays of `ProfileItem` objects.

#### Scenario: Adding a new connection type
- **WHEN** a developer adds support for a new connection type (e.g., SSH)
- **THEN** they SHALL only need to create a new resolver function and add i18n strings
- **AND** the `BottomInfoPanel` component SHALL require no code changes

#### Scenario: Serial profile resolution
- **WHEN** the active session has connection_type "serial" with params `{baud_rate: 115200, data_bits: 8, parity: "none", stop_bits: "1", flow_control: "none"}`
- **THEN** the right column parameters area SHALL display baud rate, data bits, parity, stop bits, and flow control as labeled rows

#### Scenario: Unknown protocol fallback
- **WHEN** the active session's connection type has no registered resolver
- **THEN** the parameters area SHALL display a fallback message with the raw connection type string

### Requirement: Real-time runtime statistics
The info panel SHALL display real-time I/O statistics and connection uptime for the active session.

#### Scenario: Stats update during active transfer
- **WHEN** data is being sent and received on the active session
- **THEN** the TX bytes and RX bytes SHALL update at 1-second intervals
- **AND** the uptime display SHALL increment each second

#### Scenario: Stats display when session is disconnected
- **WHEN** the active session is in "disconnected" state
- **THEN** the stats area SHALL show zero bytes and no uptime (or "--" placeholder)
- **AND** the uptime timer SHALL stop

#### Scenario: Byte count formatting
- **WHEN** TX bytes = 12,400
- **THEN** the display SHALL show "12.4 KB"
- **WHEN** TX bytes = 1,024,000
- **THEN** the display SHALL show "1.0 MB"

### Requirement: Status indicator with visual states
The connection status display SHALL use distinct visual treatments for connected, disconnected, and connecting states.

#### Scenario: Connected state indicator
- **WHEN** session state is "connected"
- **THEN** the status row SHALL show a green indicator and the translated "Connected" text

#### Scenario: Disconnected state indicator
- **WHEN** session state is "disconnected"
- **THEN** the status row SHALL show a red indicator and the translated "Disconnected" text

#### Scenario: Connecting state indicator
- **WHEN** session state is "connecting"
- **THEN** the status row SHALL show a yellow indicator with a CSS pulse animation
- **AND** the translated "Connecting" text SHALL be displayed

### Requirement: Monospace rendering for technical values
Technical parameter values SHALL be rendered in a monospace font family to distinguish them from descriptive labels.

#### Scenario: Serial parameter display
- **WHEN** the baud rate value "115200" is displayed
- **THEN** it SHALL use `var(--font-mono)` font family
- **AND** the label "Baud Rate" SHALL use the default proportional font
