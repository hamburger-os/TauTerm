# liquid-glass-theme

## Purpose

定义 Liquid Glass v3 主题系统要求，包括 Google Glow 默认主题、CSS 变量令牌系统、多主题预设和动画效果。

## Requirements

### Requirement: Google Glow Default Theme
The system SHALL provide a "Google Glow" theme as the default.

The theme SHALL use a deep dark background (`#080808`) with 4 animated Google-colored orbs (blue `#4285F4`, red `#EA4335`, yellow `#FBBC05`, green `#34A853`) as ambient background.

The theme SHALL implement enhanced glassmorphism via `backdrop-filter: blur(25px)`, SVG `feTurbulence` noise texture, and asymmetric border highlights (top/left brighter than bottom/right).

#### Scenario: Default theme on first launch
- **WHEN** application starts for the first time or localStorage has no saved theme
- **THEN** the Google Glow theme is active with 4 colored morphing orbs and semi-transparent glass panels

### Requirement: CSS Variable Token System
The system SHALL define all visual properties as CSS custom properties in `src/styles/tokens.css`.

The system SHALL support three themes via `data-theme` attribute: `google-glow` (default), `obsidian`, and `frosted`.

The token system SHALL include the following categories:

**Level 1 — Shared Constants** (unchanged across themes):
- Font families, border radii, spacing scale, transition durations, z-index layers

**Level 2 — Theme Tokens** (vary per theme):
- Background: `--bg-base`, `--bg-orb-opacity`, `--bg-orb-blur`, `--bg-orb-blend`
- Glass: `--glass-fill`, `--glass-noise-opacity`, `--glass-noise-frequency`, `--glass-blur`, `--glass-blur-saturate`, `--glass-border-default`, `--glass-border-top`, `--glass-border-left`, `--glass-shadow-outer`, `--glass-shadow-inner`
- Buttons: `--glass-button-bg`, `--glass-button-border`, `--glass-button-hover-bg`, `--glass-button-hover-border`, `--glass-button-shadow-inset`
- Inputs: `--glass-input-bg`, `--glass-input-border`, `--glass-input-shadow-inner`, `--glass-input-focus-border`, `--glass-input-focus-glow`
- Layout surfaces: `.liquid-glass` global class with `var(--glass-fill)` gradient
- Text & Accent: `--text-primary`, `--text-secondary`, `--text-muted`, `--accent-primary`, `--accent-secondary`, `--accent-gradient`, `--accent-glow`
- Status colors: `--color-success`, `--color-error`, `--color-warning`, `--color-info`
- Dialog: `--dialog-bg`, `--dialog-shadow`, `--select-option-bg`

#### Scenario: Theme token override
- **WHEN** `data-theme` attribute changes to "obsidian"
- **THEN** all UI elements using CSS variables update their appearance without re-render

#### Scenario: Theme persists across app restarts
- **WHEN** user sets theme to "obsidian" and restarts the application
- **THEN** the application loads with "obsidian" theme active

### Requirement: Glass Panel Interaction States
UI panels SHALL have three visual states:
- **Default**: glass fill gradient background, SVG noise texture overlay, asymmetric border (top/left brighter than bottom/right)
- **Hover**: background brightens, border glows with accent color, element lifts with `translateY(-2px)`
- **Active/Selected**: distinct background with brighter border and inner highlight shadow

All state transitions SHALL animate with `transition: all 0.3s cubic-bezier(0.4, 0, 0.2, 1)`.

#### Scenario: Hover over session item
- **WHEN** user hovers over a session item in the sidebar
- **THEN** the item's border glows, background brightens, and the item lifts by 2px over 0.3s

#### Scenario: Select session item
- **WHEN** user clicks a session item
- **THEN** the item shows an active state with brighter background, visible border, and inner highlight shadow

### Requirement: Status Indicator Breathing Animation
The system SHALL display a breathing animation on connection status dots:
- **Connected**: steady bright green dot with `box-shadow` glow (`0 0 10px rgba(74,222,128,0.9)`), pulsing
- **Disconnected**: dim steady, no animation
- **Transferring**: steady bright yellow dot with glow, pulsing

#### Scenario: Connection established
- **WHEN** serial connection succeeds
- **THEN** the status dot becomes bright green with a glowing halo and begins pulsing

### Requirement: Drag Handle Glow Effect
Resize handles SHALL detect mouse proximity within 10px and:
- Show a glowing accent-colored line when mouse is near
- Display a capsule icon at the handle center
- The glow SHALL fade out 0.5s after mouse leaves

#### Scenario: Mouse approaches resize handle
- **WHEN** mouse cursor moves within 10px of the terminal/panel resize handle
- **THEN** an accent-colored glowing line appears at the handle with a central capsule icon

### Requirement: Dropzone Animation
The file transfer panel SHALL act as a dropzone.
When files are dragged into the window:
- The entire window SHALL dim with a dark overlay
- The transfer panel SHALL slide up with a breathing accent-colored border
- A "Drop to Transfer" message SHALL appear centered

#### Scenario: File dragged into window
- **WHEN** user drags a file from desktop into TauTerm window
- **THEN** window dims, transfer panel border glows with accent color, "⚡ Drop to Transfer" text appears

### Requirement: Multiple Theme Presets
The system SHALL provide 3 theme presets: Google Glow (default dark), Obsidian (darker neon), Frosted (light).

Users SHALL be able to switch themes instantly via settings or command palette.

Legacy theme IDs (`neon-dark`, `ocean`, `sunset`) SHALL be migrated to `google-glow` on next launch.

#### Scenario: Theme switch via settings
- **WHEN** user opens Settings → Appearance and selects "Obsidian"
- **THEN** all UI elements transition to the Obsidian color scheme with near-black background and neon orbs

#### Scenario: Legacy theme migration
- **WHEN** application starts and localStorage has `tauterm-theme: "ocean"`
- **THEN** theme defaults to `google-glow` since "ocean" is no longer available
