# new-session-dialog (Delta)

## Purpose

修改新建会话对话框要求——模式卡片从硬编码列表改为从 Plugin Registry 动态生成。`ConnectionType::all()` 移除，协议选项完全由已注册插件驱动。

## MODIFIED Requirements

### Requirement: New session dialog has mode-first design
The new session dialog SHALL present a two-step flow: first select a protocol from the registered plugins, then configure protocol-specific parameters. The protocol selection SHALL be presented as a grid of visually distinct cards dynamically generated from the Plugin Registry.

#### Scenario: User opens the new session dialog
- **WHEN** the user clicks the "New Session" toolbar button or triggers the new-session keyboard shortcut
- **THEN** a modal dialog opens showing a grid of protocol cards derived from all plugins registered in the Plugin Host that declare the `connection` capability
- **AND** each card SHALL display the plugin's icon, name, and description from its manifest

#### Scenario: User selects the Serial protocol card
- **WHEN** the user clicks the "Serial" protocol card
- **THEN** the dialog SHALL transition to show the `SerialConnectForm` component provided by the Serial plugin via `registerPlugin()`
- **AND** the form SHALL include all fields defined in the Serial plugin's `config_schema`

#### Scenario: All displayed protocol cards are available
- **WHEN** the dialog displays protocol cards
- **THEN** every displayed card SHALL represent a fully functional, registered plugin—no "Coming Soon" badges or disabled cards
- **AND** plugins without the `connection` capability SHALL be excluded from the dialog

### Requirement: Protocol cards are dynamically generated from Plugin Registry
The dialog SHALL query the Plugin Host for all plugins declaring the `connection` capability and render one card per plugin.
The dialog SHALL NOT hard-code any protocol list or connection type enumeration.

#### Scenario: New plugin appears automatically in dialog
- **WHEN** a new protocol plugin is registered with the Plugin Host (with `connection` capability)
- **THEN** its protocol card SHALL appear in the new session dialog without any dialog code changes

#### Scenario: Plugin is removed or disabled
- **WHEN** a plugin is unregistered from the Plugin Host
- **THEN** its protocol card SHALL NOT appear in the new session dialog

### Requirement: Serial mode configuration is fully functional
The Serial configuration panel SHALL provide all fields to establish a serial connection and SHALL connect to the selected port with the specified parameters.
The Serial plugin SHALL provide the connect form component and handle parameter validation.

#### Scenario: User configures and connects via serial
- **WHEN** the user selects a serial port, sets parameters, and clicks Connect
- **THEN** the system SHALL invoke `connect_session` with `plugin_id = "serial"`, endpoint, and parameters
- **AND** upon successful connection, the dialog SHALL close
- **AND** a new session tab SHALL appear in "connecting" then "connected" state

#### Scenario: User configures but no port is available
- **WHEN** no serial ports are detected by the Serial plugin's `discover_endpoints()`
- **THEN** the port selector SHALL show "No serial ports detected"
- **AND** the Connect button SHALL be disabled

### Requirement: Session name is optional with smart default
The session name field SHALL be optional. When left empty, the system SHALL generate a default name from the plugin name and endpoint.

#### Scenario: User leaves session name empty
- **WHEN** the user connects without entering a session name
- **THEN** the session SHALL be named with the format "<PluginName> @ <Endpoint>" (e.g., "Serial @ COM3", "SSH @ 192.168.1.1")

## REMOVED Requirements

### Requirement: Mode cards support extensibility via new card components
**Reason**: The extensibility model changes from "add a card component and config panel component" to "register a plugin with `registerPlugin()` including its `connectForm` component." The dialog no longer needs internal knowledge of each protocol—it delegates entirely to plugin-provided components.
**Migration**: Existing protocol-specific card and config panel components (Serial, SSH placeholder, Telnet placeholder, TFTP placeholder) SHALL be moved into their respective plugins. The dialog SHALL use a generic `PluginCard` component that renders plugin manifest data, and render whichever `connectForm` the selected plugin provides.
