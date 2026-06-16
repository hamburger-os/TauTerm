# command-palette

## Purpose

定义命令面板功能要求，包括模糊搜索、分类组织和命令执行。

## Requirements

### Requirement: Open Command Palette
The system SHALL open a command palette overlay when user presses Ctrl+Shift+P.
The palette SHALL display a search input and a list of available commands.
The palette SHALL auto-focus the input field.
The system SHALL close the palette on Escape or when a command is executed.

#### Scenario: Open palette
- **WHEN** user presses Ctrl+Shift+P
- **THEN** a modal overlay appears with an input field and a list of available commands

### Requirement: Fuzzy Search Commands
The system SHALL filter the command list using fuzzy matching against the user's input.
The system SHALL display matching commands ranked by relevance.
Each command SHALL show its keyboard shortcut (if any) on the right side.

#### Scenario: Fuzzy search
- **WHEN** user types "disc" in the command palette
- **THEN** commands containing "disconnect" are shown with their shortcuts

#### Scenario: No matches
- **WHEN** user types text matching no commands
- **THEN** "No commands found" is displayed

### Requirement: Command Categories
The system SHALL organize commands into categories: Session, Terminal, Transfer, Theme, Application.
The system SHALL display the category name as a section header in the command list.

#### Scenario: Browse by category
- **WHEN** user opens the command palette without typing
- **THEN** commands are grouped under category headers (Session, Terminal, Transfer, Theme, Application)

### Requirement: Execute Command
The system SHALL execute the selected command when user clicks it or presses Enter.
The system SHALL close the palette after execution.
Each command SHALL have a defined action (Tauri invoke, state change, UI toggle, etc.).

#### Scenario: Execute via keyboard
- **WHEN** user types "theme" and presses Enter with "Theme: Switch to Ocean" selected
- **THEN** the theme changes to Ocean and the palette closes

### Requirement: Available Commands
The system SHALL provide at minimum these commands:
- Session: New Session, Close Session, Rename Session, Disconnect
- Terminal: Search (Ctrl+F), Clear Buffer, Copy, Paste
- Transfer: Send Files, Receive Files, Cancel Transfer
- Theme: Switch to Neon Dark, Switch to Ocean, Switch to Sunset
- Application: Toggle Sidebar, Toggle Transfer Panel, Settings

#### Scenario: Command list completeness
- **WHEN** the command palette opens
- **THEN** all listed commands are present and functional
