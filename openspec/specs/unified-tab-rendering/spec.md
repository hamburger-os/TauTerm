# unified-tab-rendering

## Purpose

定义 TauTerm 的统一标签页渲染架构，通过 `content_type` 适配器模式使所有会话类型共享同一套标签页系统，根据会话类型动态切换内容视图。

## Requirements

### Requirement: Tab bar hosts all session types uniformly
The system SHALL render all session tabs in a single tab bar regardless of session type or plugin origin.
Each tab SHALL display the plugin icon, session name, and a status indicator (connected/disconnected/transferring/error) using a colored dot.
The tab bar SHALL support drag-to-reorder, right-click context menu, and close button per tab.

#### Scenario: Serial, SSH, and FTP tabs coexist in tab bar
- **WHEN** the user has three sessions open: Serial (COM3), SSH (192.168.1.1), FTP (files.example.com)
- **THEN** all three SHALL appear in the same tab bar with their respective plugin icons and status indicators

#### Scenario: Tab status reflects session state
- **WHEN** an SSH session disconnects unexpectedly
- **THEN** its tab status indicator SHALL change to red (disconnected) and the tab name SHALL remain visible in the bar

### Requirement: Content type adapter dynamically renders views
The system SHALL select the content renderer for a tab based on its plugin's `content_type` field.
The kernel SHALL provide four built-in content renderers: `TerminalRenderer` (xterm.js instance pool), `FileBrowserRenderer` (dual-pane file tree), `StatsDashboardRenderer` (charts and gauges), and `CustomRenderer` (delegates to plugin's provided component).
The renderer SHALL be instantiated when a tab becomes active and SHALL receive the tab's session ID and plugin configuration.

#### Scenario: Terminal type renders xterm.js
- **WHEN** a tab with `content_type: "terminal"` is activated
- **THEN** the `TerminalRenderer` SHALL retrieve or create an xterm.js instance for the tab's session ID and connect it to the session's data stream

#### Scenario: File browser type renders dual-pane layout
- **WHEN** a tab with `content_type: "file_browser"` is activated
- **THEN** the `FileBrowserRenderer` SHALL render a dual-pane file tree (local on left, remote on right) with drag-and-drop support

#### Scenario: Custom type delegates to plugin component
- **WHEN** a tab with `content_type: "custom"` is activated
- **THEN** the `CustomRenderer` SHALL render the plugin's registered custom view component, passing `{ sessionId, channel, pluginConfig }` as props

### Requirement: Terminal renderer maintains instance pool
The `TerminalRenderer` SHALL maintain a pool of xterm.js instances, keeping all connected terminals alive in the DOM with CSS `opacity` controlling visibility.
Non-active terminal instances SHALL continue to receive and buffer data.
Switching between terminal tabs SHALL use CSS `opacity` transition (0.15s) without rebuilding xterm.js instances.

#### Scenario: Switching between two terminal tabs
- **WHEN** the user switches from SSH tab to Serial tab
- **THEN** the SSH terminal SHALL transition to `opacity: 0; pointer-events: none` and the Serial terminal SHALL transition to `opacity: 1`, both without re-creating xterm.js instances

#### Scenario: Terminal instance cleanup on tab close
- **WHEN** a terminal tab is closed
- **THEN** its xterm.js instance SHALL be disposed and removed from the instance pool

### Requirement: Plugin toolbar slots are context-aware
The toolbar SHALL display the active tab's plugin-registered toolbar items.
When no tab is active or tabs of different types are selected, the toolbar SHALL revert to default items.
Toolbar items SHALL support left, center, and right positioning.

#### Scenario: Serial toolbar items appear when Serial tab is active
- **WHEN** the user activates a Serial session tab
- **THEN** the toolbar SHALL show Serial-specific items (refresh ports, DTR toggle, baud rate indicator) alongside global items

#### Scenario: Toolbar reverts when switching to SSH tab
- **WHEN** the user switches from Serial to SSH tab
- **THEN** the toolbar SHALL replace Serial items with SSH-specific items (reconnect, key manager, port forwarding)

### Requirement: Bottom panel supports plugin-contributed tab pages
The bottom panel SHALL render as a tabbed interface with built-in tabs (Info, Transfer History) and plugin-contributed tabs.
The bottom panel height SHALL be resizable between a configured minimum and 50% of window height.
Each plugin-contributed bottom panel SHALL receive the active session's context.

#### Scenario: SSH plugin adds Port Forwarding panel
- **WHEN** an SSH session is active
- **THEN** the bottom panel SHALL include an "Port Forwarding" tab contributed by the SSH plugin

### Requirement: Status bar aggregates plugin status items
The status bar SHALL display, from left to right: connection status indicator, TX/RX throughput, active plugin status items, and global status (clock, keyboard layout).
Plugin status items SHALL use a `render(session_context) -> ReactNode` function for dynamic content.

#### Scenario: Serial signals appear in status bar
- **WHEN** a Serial session is active
- **THEN** the status bar SHALL display CTS/RTS/DTR signal states via the Serial plugin's status bar item
