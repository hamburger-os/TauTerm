# Liquid Glass Theme v2

## ADDED Requirements

### Requirement: Neon Dark Theme
The system SHALL provide a "Neon Dark" theme as the default.
The theme SHALL use a deep dark background (`#060610`) with cyan (`#00d4aa`) and blue (`#00a3ff`) accent colors.
The theme SHALL implement glassmorphism via `backdrop-filter: blur()` and semi-transparent backgrounds.

#### Scenario: Default theme
- **WHEN** application starts for the first time
- **THEN** the Neon Dark theme is active with glass panels, cyan accents, and dark background

### Requirement: CSS Variable Token System
The system SHALL define all visual properties as CSS custom properties in a token file.
The system SHALL support runtime theme switching via `data-theme` attribute.
Tokens SHALL include: backgrounds, glass panels, borders, text colors, accent colors, blur amounts, border radii, transitions, spacing.

#### Scenario: Theme token override
- **WHEN** `data-theme` attribute changes to "ocean"
- **THEN** all UI elements using CSS variables update their appearance without re-render

### Requirement: Glass Panel Interaction States
UI panels SHALL have three visual states:
- **Default**: semi-transparent background, subtle border
- **Hover**: background brightens, border glows with accent color, blur increases
- **Active/Selected**: left border shows 3px colored indicator, background slightly more opaque

All state transitions SHALL animate with `transition: all 0.3s ease`.

#### Scenario: Hover over session item
- **WHEN** user hovers over a session item in the sidebar
- **THEN** the item's border glows cyan, background brightens over 0.3s

#### Scenario: Select session item
- **WHEN** user clicks a session item
- **THEN** the item shows a 3px cyan left border indicator with a subtle scale(0.98) click feedback

### Requirement: Status Indicator Breathing Animation
The system SHALL display a breathing animation on connection status dots:
- Connecting: 1.5s pulse cycle from dim to bright
- Connected: steady bright with a one-time ripple effect on connection
- Disconnected: dim steady, no animation

#### Scenario: Connection established
- **WHEN** serial connection succeeds
- **THEN** the status dot becomes bright cyan and emits a ripple animation that fades outward

### Requirement: Drag Handle Glow Effect
Resize handles SHALL detect mouse proximity within 10px and:
- Show a glowing cyan line when mouse is near
- Display a capsule icon (===) at the handle center
- The glow SHALL fade out 0.5s after mouse leaves

#### Scenario: Mouse approaches resize handle
- **WHEN** mouse cursor moves within 10px of the terminal/panel resize handle
- **THEN** a cyan glowing line appears at the handle with a central capsule icon

### Requirement: Dropzone Animation
The file transfer panel SHALL act as a dropzone.
When files are dragged into the window:
- The entire window SHALL dim with a dark overlay
- The transfer panel SHALL slide up with a breathing cyan border
- A "Drop to Transfer" message SHALL appear centered
- On drop, a scan-line sweep animation SHALL play across the panel

#### Scenario: File dragged into window
- **WHEN** user drags a .bin file from desktop into TauTerm window
- **THEN** window dims, transfer panel breathes with cyan glow, "⚡ Drop to Transfer" text appears

### Requirement: Multiple Theme Presets
The system SHALL provide at least 3 theme presets: Neon Dark, Ocean Blue, Sunset Amber.
Users SHALL be able to switch themes instantly via settings or command palette.

#### Scenario: Theme switch via command palette
- **WHEN** user opens command palette and selects "Theme: Ocean Blue"
- **THEN** all UI elements transition to the Ocean Blue color scheme
