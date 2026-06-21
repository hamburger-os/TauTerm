# liquid-glass-theme (delta)

## Purpose

修改 Liquid Glass 主题系统——删除 neon-dark / ocean / sunset 三套 v2 主题，替换为 google-glow / obsidian / frosted 三套 v3 主题；扩展 CSS 自定义属性令牌以支持光球参数、噪点纹理、不对称边框、增强玻璃效果。

## REMOVED Requirements

### Requirement: Neon Dark Theme
**Reason**: Replaced by google-glow as the new default theme with dynamic orb background and enhanced glass effects.
**Migration**: User localStorage key `tauterm-theme` value "neon-dark" will be treated as "google-glow" on next launch. No data loss.

### Requirement: Multiple Theme Presets
**Reason**: The three theme presets (Neon Dark, Ocean Blue, Sunset Amber) are replaced by three new presets (Google Glow, Obsidian, Frosted).
**Migration**: Users who selected "ocean" or "sunset" will be reset to the new default "google-glow" theme. The Appearance settings panel will show the new theme options.

## MODIFIED Requirements

### Requirement: CSS Variable Token System
The system SHALL define all visual properties as CSS custom properties in `src/styles/tokens.css`.

The system SHALL support three themes via `data-theme` attribute: `google-glow` (default), `obsidian`, and `frosted`.

The token system SHALL include the following categories, extended from v2:

**Level 1 — Shared Constants** (unchanged across themes):
- Font families (`--font-ui: "Inter"`, `--font-mono: "JetBrains Mono"`)
- Border radii (`--radius-sm: 4px` through `--radius-full: 9999px`, plus `--radius-2xl: 24px` for 1.5rem main window)
- Spacing scale (`--spacing-xs: 4px` through `--spacing-2xl: 32px`)
- Transition durations (`--transition-fast: 150ms`, `--transition-normal: 300ms`, `--transition-slow: 500ms`)
- Z-index layers (`--z-sidebar`, `--z-panel`, `--z-overlay`, `--z-toast`)

**Level 2 — Theme Tokens** (vary per theme):
- Background: `--bg-base`, `--bg-orb-opacity`, `--bg-orb-blur`, `--bg-orb-blend`
- Glass: `--glass-fill`, `--glass-noise-opacity`, `--glass-noise-frequency`, `--glass-blur`, `--glass-blur-saturate`, `--glass-border-default`, `--glass-border-top`, `--glass-border-left`, `--glass-shadow-outer`, `--glass-shadow-inner`
- Buttons: `--glass-button-bg`, `--glass-button-border`, `--glass-button-hover-bg`, `--glass-button-hover-border`, `--glass-button-shadow-inset`
- Inputs: `--glass-input-bg`, `--glass-input-border`, `--glass-input-shadow-inner`, `--glass-input-focus-border`, `--glass-input-focus-glow`
- Layout blocks: `--block-toolbar-bg`, `--block-sidebar-bg`, `--block-terminal-bg`, `--block-sendbar-bg`
- Text & Accent: `--text-primary`, `--text-secondary`, `--text-muted`, `--accent-primary`, `--accent-secondary`, `--accent-gradient`, `--accent-glow`
- Status colors: `--color-success`, `--color-error`, `--color-warning`, `--color-info`

**Theme-specific values**:

**google-glow**:
- `--bg-base`: `#080808`; `--bg-orb-opacity`: `0.65`; `--bg-orb-blur`: `120px`; `--bg-orb-blend`: `screen`
- `--glass-fill`: `linear-gradient(135deg, rgba(255,255,255,0.08), rgba(255,255,255,0.02))`
- `--glass-noise-opacity`: `0.04`; `--glass-noise-frequency`: `0.8`
- `--glass-blur`: `25px`; `--glass-blur-saturate`: `100%`
- `--glass-border-default`: `rgba(255,255,255,0.15)`; `--glass-border-top`: `rgba(255,255,255,0.25)`; `--glass-border-left`: `rgba(255,255,255,0.2)`
- `--glass-shadow-outer`: `0 10px 40px 0 rgba(0,0,0,0.4)`; `--glass-shadow-inner`: `inset 0 1px 0 0 rgba(255,255,255,0.15)`
- `--glass-button-bg`: `rgba(255,255,255,0.05)`; `--glass-button-border`: `rgba(255,255,255,0.1)`; `--glass-button-hover-bg`: `rgba(255,255,255,0.15)`; `--glass-button-hover-border`: `rgba(255,255,255,0.3)`
- `--glass-input-bg`: `rgba(0,0,0,0.25)`; `--glass-input-border`: `rgba(255,255,255,0.05)`; `--glass-input-shadow-inner`: `inset 0 2px 5px rgba(0,0,0,0.3)`; `--glass-input-focus-border`: `rgba(255,255,255,0.25)`; `--glass-input-focus-glow`: `0 0 20px rgba(66,133,244,0.4)`
- `--block-toolbar-bg`: `rgba(0,0,0,0.1)`; `--block-sidebar-bg`: `rgba(0,0,0,0.2)`; `--block-terminal-bg`: `rgba(0,0,0,0.3)`; `--block-sendbar-bg`: `rgba(0,0,0,0.4)`
- `--text-primary`: `#e0e0ff`; `--text-secondary`: `#8888aa`; `--text-muted`: `#505070`
- `--accent-primary`: `#4285F4`; `--accent-secondary`: `#60a5fa`; `--accent-gradient`: `linear-gradient(135deg, #4285F4, #6366f1)`; `--accent-glow`: `rgba(66,133,244,0.35)`
- `--color-success`: `#34d399`; `--color-error`: `#ff4757`; `--color-warning`: `#ffa502`; `--color-info`: `#4285F4`

**obsidian**:
- `--bg-base`: `#030303`; `--bg-orb-opacity`: `0.45`; `--bg-orb-blur`: `140px`; `--bg-orb-blend`: `screen`
- `--glass-fill`: `linear-gradient(135deg, rgba(20,20,25,0.6), rgba(5,5,10,0.4))`
- `--glass-noise-opacity`: `0.05`; `--glass-noise-frequency`: `0.85`
- `--glass-blur`: `30px`; `--glass-blur-saturate`: `120%`
- `--glass-border-default`: `rgba(255,255,255,0.05)`; `--glass-border-top`: `rgba(255,255,255,0.12)`; `--glass-border-left`: `rgba(255,255,255,0.08)`
- `--glass-shadow-outer`: `0 20px 50px 0 rgba(0,0,0,0.7)`; `--glass-shadow-inner`: `inset 0 1px 0 0 rgba(255,255,255,0.05)`
- `--glass-button-bg`: `rgba(0,0,0,0.4)`; `--glass-button-border`: `rgba(255,255,255,0.05)`; `--glass-button-hover-bg`: `rgba(40,40,45,0.6)`; `--glass-button-hover-border`: `rgba(255,255,255,0.15)`
- `--glass-input-bg`: `rgba(0,0,0,0.6)`; `--glass-input-border`: `rgba(255,255,255,0.03)`; `--glass-input-shadow-inner`: `inset 0 2px 8px rgba(0,0,0,0.8)`; `--glass-input-focus-border`: `rgba(255,255,255,0.15)`; `--glass-input-focus-glow`: `0 0 20px rgba(66,133,244,0.3)`
- `--block-toolbar-bg`: `rgba(0,0,0,0.4)`; `--block-sidebar-bg`: `rgba(0,0,0,0.6)`; `--block-terminal-bg`: `rgba(0,0,0,0.5)`; `--block-sendbar-bg`: `rgba(0,0,0,0.6)`
- `--text-primary`: `rgba(255,255,255,0.9)`; `--text-secondary`: `rgba(255,255,255,0.6)`; `--text-muted`: `rgba(255,255,255,0.3)`
- `--accent-primary`: `#4285F4`; `--accent-secondary`: `#60a5fa`; `--accent-gradient`: `linear-gradient(135deg, #3b82f6, #6366f1)`
- `--color-success`: `#34d399`; `--color-error`: `#ff4757`; `--color-warning`: `#ffa502`; `--color-info`: `#3b82f6`

**frosted**:
- `--bg-base`: `#f8fafc`; `--bg-orb-opacity`: `0.35`; `--bg-orb-blur`: `140px`; `--bg-orb-blend`: `multiply`
- `--glass-fill`: `linear-gradient(135deg, rgba(255,255,255,0.7), rgba(255,255,255,0.4))`
- `--glass-noise-opacity`: `0.03`; `--glass-noise-frequency`: `0.9`
- `--glass-blur`: `35px`; `--glass-blur-saturate`: `150%`
- `--glass-border-default`: `rgba(255,255,255,0.4)`; `--glass-border-top`: `rgba(255,255,255,0.8)`; `--glass-border-left`: `rgba(255,255,255,0.6)`
- `--glass-shadow-outer`: `0 20px 50px 0 rgba(0,0,0,0.08)`; `--glass-shadow-inner`: `inset 0 2px 5px 0 rgba(255,255,255,0.5)`
- `--glass-button-bg`: `rgba(255,255,255,0.6)`; `--glass-button-border`: `rgba(255,255,255,0.5)`; `--glass-button-hover-bg`: `rgba(255,255,255,0.9)`; `--glass-button-hover-border`: `rgba(255,255,255,0.9)`
- `--glass-input-bg`: `rgba(255,255,255,0.5)`; `--glass-input-border`: `rgba(255,255,255,0.4)`; `--glass-input-shadow-inner`: `inset 0 2px 6px rgba(0,0,0,0.05)`; `--glass-input-focus-border`: `rgba(66,133,244,0.4)`; `--glass-input-focus-glow`: `0 0 0 3px rgba(66,133,244,0.15)`
- `--block-toolbar-bg`: `rgba(255,255,255,0.3)`; `--block-sidebar-bg`: `rgba(255,255,255,0.2)`; `--block-terminal-bg`: `rgba(255,255,255,0.3)`; `--block-sendbar-bg`: `rgba(255,255,255,0.5)`
- `--text-primary`: `#1e293b`; `--text-secondary`: `#475569`; `--text-muted`: `#94a3b8`
- `--accent-primary`: `#3b82f6`; `--accent-secondary`: `#60a5fa`; `--accent-gradient`: `linear-gradient(135deg, #3b82f6, #6366f1)`
- `--color-success`: `#16a34a`; `--color-error`: `#dc2626`; `--color-warning`: `#d97706`; `--color-info`: `#2563eb`

Theme switching SHALL update `document.documentElement.dataset.theme` and persist the selected theme ID to `localStorage` key `tauterm-theme`.

#### Scenario: Default theme on first launch
- **WHEN** application starts for the first time or localStorage has no saved theme
- **THEN** the `google-glow` theme is active with Google 4-color orbs and semi-transparent glass panels

#### Scenario: Theme switch to obsidian
- **WHEN** user selects "Obsidian" theme
- **THEN** all CSS custom properties switch to obsidian values: background darkens to near-black, glass panels take on smoked dark gradient, orbs become more neon-like with reduced opacity

#### Scenario: Theme switch to frosted
- **WHEN** user selects "Frosted" theme
- **THEN** all CSS custom properties switch to frosted values: background becomes light gray-white, glass panels become bright and translucent, text darkens, orbs use multiply blend mode

#### Scenario: Theme persists across app restarts
- **WHEN** user sets theme to "obsidian" and restarts the application
- **THEN** the application loads with "obsidian" theme active

### Requirement: Glass Panel Interaction States
UI panels SHALL have three visual states:
- **Default**: glass fill gradient background, SVG noise texture overlay, asymmetric border (top/left brighter than bottom/right)
- **Hover**: background brightens, border glows with accent color, blur increases, element lifts with `translateY(-2px)`
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
- **Transferring**: steady bright with glow

#### Scenario: Connection established
- **WHEN** serial connection succeeds
- **THEN** the status dot becomes bright green with a glowing halo and begins pulsing

### Requirement: Dropzone Animation (unchanged from v2)
The file transfer panel SHALL act as a dropzone.
When files are dragged into the window:
- The entire window SHALL dim with a dark overlay
- The transfer panel SHALL slide up with a breathing accent-colored border
- A "Drop to Transfer" message SHALL appear centered

#### Scenario: File dragged into window
- **WHEN** user drags a file from desktop into TauTerm window
- **THEN** window dims, transfer panel border glows with accent color, "⚡ Drop to Transfer" text appears
