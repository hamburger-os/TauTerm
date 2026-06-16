# term-search (delta)

## MODIFIED Requirements

### Requirement: Text Search
The system SHALL search the terminal's scrollback buffer for the entered text.
The system SHALL use xterm.js's built-in search addon (`@xterm/addon-search`) or programmatic terminal buffer scanning to highlight all matching occurrences in the visible terminal area and scroll through the full scrollback.
The system SHALL display the current match index and total match count.

#### Scenario: Search with matches
- **WHEN** user types "error" in the search bar and the terminal contains 5 occurrences
- **THEN** all 5 occurrences are visually highlighted in the terminal viewport, the first match is selected (distinct highlight color), the viewport scrolls to show it, and "1/5" is displayed

#### Scenario: Search with no matches
- **WHEN** user types text that does not exist in the buffer
- **THEN** the input border turns red/orange, "0/0" is displayed, and no highlights are shown

#### Scenario: Search across scrollback
- **WHEN** user searches for text that exists only in the scrollback buffer (not currently visible)
- **THEN** the viewport SHALL scroll to show the first matching occurrence

### Requirement: Navigation Between Matches
The system SHALL support navigating between matches:
- Enter / Arrow Down: move to next match
- Shift+Enter / Arrow Up: move to previous match
The viewport SHALL scroll to bring the selected match into view.
The active match SHALL use a distinct highlight color to distinguish it from inactive matches.

#### Scenario: Navigate to next match
- **WHEN** user presses Enter and there is a next match
- **THEN** the viewport scrolls to show the next highlighted match, counter updates to "2/5", and the previous match reverts to inactive highlight

#### Scenario: Wrap to first match
- **WHEN** user navigates past the last match
- **THEN** the selection wraps to the first match, counter shows "1/5"

#### Scenario: Previous match keyboard shortcut
- **WHEN** user presses Shift+Enter
- **THEN** the viewport scrolls to show the previous match and counter decrements

### Requirement: Case Sensitivity Toggle
The system SHALL provide a case-sensitive toggle button in the search bar.
The system SHALL re-execute the search when toggled.

#### Scenario: Toggle case sensitivity
- **WHEN** user toggles case-sensitive search and types "Error"
- **THEN** only "Error" (not "error" or "ERROR") is matched and highlighted
