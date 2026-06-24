# plugin-system

## Purpose

定义 TauTerm 插件系统的清单格式、核心 Trait、能力声明和完整生命周期。插件是 TauTerm 中唯一的功能提供方式——内核不包含任何协议实现。

## Requirements

### Requirement: Plugin manifest defines plugin metadata
Every plugin SHALL provide a `manifest.json` file containing `id`, `name`, `version`, `category`, `description`, `icon`, `content_type`, and `config_schema` fields.
The `content_type` field SHALL be one of: `"terminal"`, `"file_browser"`, `"stats_dashboard"`, or `"custom"`.
The `config_schema` field SHALL be a valid JSON Schema describing the plugin's connection parameters.

#### Scenario: Valid manifest is loaded
- **WHEN** the Plugin Host discovers a plugin with a valid manifest
- **THEN** the plugin SHALL be added to the registry and made available for session creation

#### Scenario: Manifest with unknown content_type
- **WHEN** a plugin declares `content_type: "unknown_type"`
- **THEN** the Plugin Host SHALL reject the plugin and log a warning

### Requirement: Plugin declares capabilities
Every plugin SHALL provide a capabilities declaration enumerating which system capabilities it requires.
The system SHALL support the following capability tokens: `connection`, `transfer`, `endpoint_discovery`, `stream`, `authentication`, `credential_store`, `filesystem_access`, `network_outbound`, `network_listen`.
Plugins SHALL only access capabilities they have declared.

#### Scenario: Plugin declares transfer capability
- **WHEN** the Serial plugin declares `capabilities: ["connection", "transfer", "endpoint_discovery"]`
- **THEN** the Transfer Manager SHALL enable transfer UI for Serial sessions

#### Scenario: Plugin attempts undeclared capability
- **WHEN** a plugin without `network_outbound` capability attempts to open a TCP connection
- **THEN** the kernel SHALL reject the operation with a capability error

### Requirement: ProtocolAdapter trait defines the plugin backend contract
Every protocol plugin SHALL implement the `ProtocolAdapter` trait with five methods: `connect()`, `disconnect()`, `discover_endpoints()`, `content_type()`, and `transfer_protocols()`.
The `connect()` method SHALL accept an endpoint string and parameters JSON, returning a `Result<Box<dyn Channel>, SessionError>`.
The `discover_endpoints()` method SHALL return a list of available endpoints for the protocol, or an empty list if endpoint enumeration is not applicable.

#### Scenario: ProtocolAdapter connect succeeds
- **WHEN** the Serial plugin's `connect("COM3", params)` is called with valid parameters
- **THEN** the method SHALL open the serial port, wrap it in a `SerialChannel`, and return `Ok(channel)`

#### Scenario: ProtocolAdapter connect fails
- **WHEN** the SSH plugin's `connect("192.168.1.1:22", params)` is called with an unreachable host
- **THEN** the method SHALL return `Err(SessionError::ConnectionFailed { reason })` with a descriptive reason

### Requirement: Plugin lifecycle follows discover-load-init-ready-stop-unload sequence
The Plugin Host SHALL manage plugins through a defined lifecycle: Discover → Load → Initialize → Ready → (optional) Stop → Unload.
During Initialize, the plugin SHALL register its IPC commands, UI components, shortcuts, and i18n resources with the kernel.
During Stop, the plugin SHALL close all active sessions, clean up resources, and unsubscribe from events.

#### Scenario: Normal plugin lifecycle
- **WHEN** the application starts
- **THEN** all built-in plugins SHALL transition through Discover → Load → Initialize → Ready in order

#### Scenario: Plugin stop during shutdown
- **WHEN** the application exits
- **THEN** the Plugin Host SHALL call `stop()` on each active plugin, which SHALL close all sessions and clean up resources

### Requirement: Plugin provides frontend UI components via registerPlugin()
Every plugin SHALL provide frontend UI registration through `registerPlugin()` including: `connectForm` (React component for session configuration), optional `toolbarItems`, optional `contextMenuItems`, optional `bottomPanels`, and optional `statusBarItems`.
The `connectForm` component SHALL receive `(params, onChange)` props and render the protocol-specific configuration form.

#### Scenario: ConnectDialog renders plugin's connect form
- **WHEN** the user selects "Serial" in the ConnectDialog protocol list
- **THEN** the dialog SHALL render the `SerialConnectForm` component provided by the Serial plugin

#### Scenario: Active tab shows plugin toolbar items
- **WHEN** a Serial session tab is active
- **THEN** the toolbar SHALL display the Serial plugin's registered `toolbarItems` (e.g., refresh ports, DTR toggle)

### Requirement: Plugin namespace isolation prevents conflicts
Each plugin SHALL operate within an isolated namespace for i18n keys, config keys, shortcut scopes, and event subscriptions.
Plugin A SHALL NOT be able to read Plugin B's config values or subscribe to Plugin B's internal events.

#### Scenario: i18n key isolation
- **WHEN** the Serial plugin uses translation key `serial:baud_rate` and the SSH plugin uses `ssh:baud_rate`
- **THEN** both keys SHALL resolve independently without collision
