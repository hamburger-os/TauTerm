# shortcut-system

## Purpose

定义快捷键系统要求，包括统一注册、全局键盘监听、冲突检测和默认快捷键。

## Requirements

### Requirement: Unified Shortcut Registration
The system SHALL define all keyboard shortcuts in a central registry.
Each shortcut SHALL have: a unique id, default key combination, description, and associated action.
The system SHALL prevent duplicate shortcut registrations.

#### Scenario: Register a shortcut
- **WHEN** a component registers shortcut "terminal.search" with default "Ctrl+F"
- **THEN** the shortcut is available and listed in the command palette

### Requirement: Global Keyboard Listener
The system SHALL listen for keyboard events at the document level.
The system SHALL match key combinations against the registry and execute the associated action.
The system SHALL NOT trigger shortcuts when the user is typing in an input field (excluding the terminal).

#### Scenario: Trigger shortcut
- **WHEN** user presses Ctrl+Shift+F while focused in the terminal
- **THEN** the file transfer panel toggles

#### Scenario: Shortcut ignored in input field
- **WHEN** user presses Ctrl+F while focused in the session search input
- **THEN** the browser's native find behavior occurs (shortcut is not intercepted)

### Requirement: Shortcut Conflict Detection
The system SHALL detect when a new shortcut registration conflicts with an existing one.
The system SHALL log a warning when conflicts are detected.

#### Scenario: Duplicate shortcut registration
- **WHEN** two components register "Ctrl+S" for different actions
- **THEN** a warning is logged and the last registration wins

### Requirement: Default Shortcuts
The system SHALL provide these default shortcuts:
- Ctrl+Shift+N: New session
- Ctrl+Shift+W: Close active session
- Ctrl+Shift+F: Toggle file transfer panel
- Ctrl+Shift+P: Command palette
- Ctrl+F: Search in terminal
- Ctrl+Shift+R: Refresh port list
- Ctrl+Shift+B: Toggle session sidebar
- Ctrl+Tab: Next tab
- Ctrl+Shift+Tab: Previous tab
- Alt+1-9: Switch to tab by index

#### Scenario: Default shortcuts are functional
- **WHEN** user presses any of the default shortcut combinations
- **THEN** the corresponding action is executed
