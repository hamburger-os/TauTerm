# term-search

## Purpose

定义终端搜索功能要求，包括搜索栏激活、文本搜索、匹配导航和大小写切换。

## Requirements

### Requirement: Search Bar Activation
The system SHALL open a search bar overlay anchored to the top-right of the active terminal when user presses Ctrl+F.
The search bar SHALL include a text input, match counter, and navigation buttons.
The search bar SHALL auto-focus the input field.

#### Scenario: Open search
- **WHEN** user presses Ctrl+F while focused in the terminal
- **THEN** a search bar appears at the top-right of the terminal with an empty input field

#### Scenario: Close search
- **WHEN** user presses Escape while search bar is open
- **THEN** the search bar closes, all highlights are cleared

### Requirement: Text Search
The system SHALL search the terminal's scrollback buffer for the entered text.
The system SHALL highlight all matching occurrences in the visible terminal area.
The system SHALL display the current match index and total match count.

#### Scenario: Search with matches
- **WHEN** user types "error" in the search bar and the terminal contains 5 occurrences
- **THEN** all 5 occurrences are highlighted, and "1/5" is displayed

#### Scenario: Search with no matches
- **WHEN** user types text that does not exist in the buffer
- **THEN** the input border turns red/orange and "0/0" is displayed

### Requirement: Navigation Between Matches
The system SHALL support navigating between matches:
- Enter / Arrow Down: move to next match
- Shift+Enter / Arrow Up: move to previous match
The viewport SHALL scroll to bring the selected match into view.

#### Scenario: Navigate to next match
- **WHEN** user presses Enter and there is a next match
- **THEN** the viewport scrolls to show the next highlighted match, counter updates to "2/5"

#### Scenario: Wrap to first match
- **WHEN** user navigates past the last match
- **THEN** the selection wraps to the first match

### Requirement: Case Sensitivity Toggle
The system SHALL provide a case-sensitive toggle button in the search bar.
The system SHALL re-execute the search when toggled.

#### Scenario: Toggle case sensitivity
- **WHEN** user toggles case-sensitive search and types "Error"
- **THEN** only "Error" (not "error" or "ERROR") is matched and highlighted
