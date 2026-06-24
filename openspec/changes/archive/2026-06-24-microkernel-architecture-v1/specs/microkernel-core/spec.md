# microkernel-core

## Purpose

定义 TauTerm 微内核架构的 8 个核心模块及其职责边界。内核不包含任何协议实现、任何业务 UI 组件、任何会话类型逻辑。内核只提供平台能力，所有功能由插件提供。

## ADDED Requirements

### Requirement: Window Manager provides multi-window lifecycle
The system SHALL manage application window creation, closing, layout persistence, and split-pane state through a centralized WindowManager module.
The WindowManager SHALL support saving and restoring window layouts to the Config Store.
The WindowManager SHALL expose `open_window()`, `close_window()`, `split_pane()`, and `save_layout()` interfaces.

#### Scenario: Restore window layout on startup
- **WHEN** the application starts and a saved layout exists in Config Store
- **THEN** the WindowManager SHALL restore window positions, sizes, and split configurations

#### Scenario: Split pane creation
- **WHEN** the user triggers a split-pane action on an active tab
- **THEN** the WindowManager SHALL create a new pane adjacent to the current one, hosting a new empty tab slot

### Requirement: Tab Host manages all tab lifecycle
The system SHALL manage tab creation, activation, closing, drag-reorder, and session association through a centralized TabHost module.
The TabHost SHALL support tabs of any `content_type` without knowing the specifics of any protocol.
The TabHost SHALL emit lifecycle events (`tab-created`, `tab-closed`, `tab-activated`) on the IPC Bridge event bus.

#### Scenario: Create a tab from a plugin
- **WHEN** a plugin calls `tab_host.create_tab(plugin_id, config)`
- **THEN** the TabHost SHALL create a new tab entry with a unique ID, associate it with the plugin, set state to "connecting", and add it to the tab bar

#### Scenario: Close a tab
- **WHEN** the user closes a tab
- **THEN** the TabHost SHALL request the associated plugin to disconnect, remove the tab from the bar, and emit `tab-closed`

#### Scenario: Drag-reorder tabs
- **WHEN** the user drags a tab to a new position in the tab bar
- **THEN** the TabHost SHALL persist the new tab order and emit `tabs-reordered`

### Requirement: IPC Bridge routes all frontend-backend communication
The system SHALL provide a centralized IPC Bridge that handles Tauri `invoke` command routing, event publishing/subscription, and binary stream channels.
The IPC Bridge SHALL allow plugins to register custom Tauri commands without modifying kernel code.
The IPC Bridge SHALL support typed event payloads validated against JSON Schema.

#### Scenario: Plugin registers a custom command
- **WHEN** a plugin calls `ipc_bridge.register_command("my_command", handler)`
- **THEN** the IPC Bridge SHALL add the command to the Tauri invoke handler and route calls to the plugin's handler function

#### Scenario: Frontend subscribes to a typed event
- **WHEN** the frontend calls `listen("session-stats", callback)`
- **THEN** the IPC Bridge SHALL deliver `session-stats` events with validated payloads matching the SessionStats schema

### Requirement: Config Store provides type-safe persistent configuration
The system SHALL provide a Config Store that supports typed read/write operations, JSON Schema validation, change notifications, and namespace isolation per plugin.
The Config Store SHALL persist to the Tauri app data directory using a structured format.
The Config Store SHALL support `get::<T>(key)`, `set(key, value)`, `watch(key, callback)`, and `delete(key)` operations.

#### Scenario: Plugin reads its configuration
- **WHEN** the Serial plugin calls `config_store.get::<SerialConfig>("serial.defaults")`
- **THEN** the Config Store SHALL return the deserialized `SerialConfig` struct or a default value if not set

#### Scenario: Configuration change triggers callback
- **WHEN** the theme setting changes via `config_store.set("theme.active", "ocean")`
- **THEN** the Config Store SHALL notify all watchers of the "theme.active" key

### Requirement: Plugin Host manages plugin discovery and lifecycle
The system SHALL provide a Plugin Host that discovers, loads, initializes, activates, deactivates, and unloads plugins.
The Plugin Host SHALL maintain a registry of active plugins keyed by `plugin_id`.
The Plugin Host SHALL expose `register_plugin(manifest, adapter, ui_components)` for built-in plugins and support future dynamic loading.

#### Scenario: Plugin Host discovers built-in plugins at startup
- **WHEN** the application starts
- **THEN** the Plugin Host SHALL load all built-in plugin manifests, call each adapter's `init()` method, and populate the plugin registry

#### Scenario: Query available connection types
- **WHEN** the frontend requests available connection types for the Connect Dialog
- **THEN** the Plugin Host SHALL return a list derived from all registered plugins that declare the `connection` capability

### Requirement: Theme Engine manages visual theming
The system SHALL provide a Theme Engine that generates CSS custom properties, supports theme switching at runtime, and allows plugins to register custom token sets.
The Theme Engine SHALL support at minimum three built-in themes: Neon Dark, Ocean, and Sunset (existing Liquid Glass v2 themes).

#### Scenario: User switches theme
- **WHEN** the user selects "Ocean" theme
- **THEN** the Theme Engine SHALL update all CSS custom properties and notify active plugins of the theme change

#### Scenario: Plugin registers custom tokens
- **WHEN** the FTP plugin calls `theme_engine.register_tokens("ftp", { "ftp-local-color": "#ff6600" })`
- **THEN** the Theme Engine SHALL merge these tokens into the active theme's CSS custom properties under the `--ftp-` namespace

### Requirement: Shortcut Engine manages keyboard shortcuts globally
The system SHALL provide a Shortcut Engine with global and plugin-scoped shortcut registration, conflict detection, and scope-based dispatch.
The active tab's plugin SHALL receive shortcut priority for its scoped bindings.

#### Scenario: Global shortcut triggers regardless of active tab
- **WHEN** the user presses `Ctrl+Shift+P` with any tab active
- **THEN** the Shortcut Engine SHALL open the Command Palette

#### Scenario: Plugin-scoped shortcut only fires when its tab is active
- **WHEN** the Serial plugin registers `Ctrl+D` for DTR toggle, and the Serial tab is not active
- **THEN** the Shortcut Engine SHALL NOT dispatch `Ctrl+D` to the Serial plugin

### Requirement: i18n Engine provides namespaced internationalization
The system SHALL provide an i18n Engine that supports namespace-isolated translation resources, plugin-contributed locale files, and runtime language switching.
Each plugin SHALL register its own translation namespace to avoid key collisions.

#### Scenario: Plugin registers translations
- **WHEN** the SSH plugin registers locale files for `zh-CN` and `en-US` under the `ssh` namespace
- **THEN** the i18n Engine SHALL make `t("ssh:connect_button")` available throughout the application

#### Scenario: Runtime language switch
- **WHEN** the user switches from `zh-CN` to `en-US`
- **THEN** the i18n Engine SHALL update all registered namespaces and re-render all UI components
