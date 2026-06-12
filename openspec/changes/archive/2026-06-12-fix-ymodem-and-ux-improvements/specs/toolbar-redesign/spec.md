## ADDED Requirements

### Requirement: Toolbar with function buttons

The application SHALL display a toolbar at the top of the window containing icon+text buttons for common operations. The quick connect input field SHALL be removed.

The toolbar SHALL include the following buttons:
- **New Session** (+ 图标) — opens the connect dialog
- **Refresh Ports** (⟳ 图标) — rescans serial ports
- **Toggle Panel** (⬆ 图标) — toggles the file transfer bottom panel
- **Toggle Sidebar** (☰ 图标) — shows or hides the session sidebar
- **Command Palette** (⌘ 图标) — opens the command palette

#### Scenario: Toolbar renders all buttons

- **WHEN** the application starts
- **THEN** a horizontal toolbar is visible at the top with all 5 function buttons, each displaying an icon and short label text

#### Scenario: Toolbar button triggers action

- **WHEN** user clicks "New Session" button
- **THEN** the connect dialog opens

#### Scenario: Quick connect input is removed

- **WHEN** the application renders
- **THEN** no text input field for quick connect address entry is present in the toolbar area

### Requirement: Toolbar i18n labels

Each toolbar button label SHALL be translatable via i18n keys (`en-US.json` and `zh-CN.json`).

#### Scenario: English labels render

- **WHEN** language is set to English
- **THEN** toolbar buttons display English labels: "New Session", "Refresh", "Transfer", "Sidebar", "Commands"

#### Scenario: Chinese labels render

- **WHEN** language is set to Chinese
- **THEN** toolbar buttons display Chinese labels: "新建会话", "刷新端口", "文件传输", "侧栏", "命令"
