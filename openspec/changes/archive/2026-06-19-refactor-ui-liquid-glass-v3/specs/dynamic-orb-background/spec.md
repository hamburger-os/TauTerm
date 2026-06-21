# dynamic-orb-background

## Purpose

定义动态光球背景系统——4 个 Google 四色光球渲染在页面最底层，应用 CSS `@keyframes` 不规则形变（morph）和盘旋运动（flow）动画，根据主题切换光球透明度、模糊半径和颜色混合模式。

## ADDED Requirements

### Requirement: Dynamic Orb Background Rendering
The system SHALL render 4 colored orbs as the application's background layer at `z-index: 0`.

The orbs SHALL use Google brand colors: Blue (`#4285F4`), Red (`#EA4335`), Yellow (`#FBBC05`), Green (`#34A853`).

Each orb SHALL be an absolutely positioned `div` with the `.glow-orb` CSS class.

The background container SHALL have `position: fixed`, `inset: 0`, `overflow: hidden`, `pointer-events: none`.

#### Scenario: Orbs render on app start
- **WHEN** the application initializes
- **THEN** 4 colored orbs appear behind all UI elements as the ambient background layer

#### Scenario: Orbs do not intercept user interaction
- **WHEN** user clicks anywhere in the application
- **THEN** clicks pass through the orb background layer to underlying UI elements

### Requirement: Orb Morphing Animation
Each orb SHALL animate its `border-radius` property using the `@keyframes morph` animation.

The morph animation SHALL cycle through irregular percentage combinations including `40% 60% 70% 30% / 40% 40% 60% 50%`, `70% 30% 50% 50% / 30% 30% 70% 70%`, and `100% 60% 60% 100% / 100% 100% 60% 60%`.

Each orb SHALL use a different animation duration (12s, 15s, 16s, 18s) and alternating direction to create organic variation.

#### Scenario: Orbs continuously morph
- **WHEN** the application is running
- **THEN** each orb's border-radius animates continuously in a non-repeating organic pattern

#### Scenario: Orbs morph at different rates
- **WHEN** observing the 4 orbs simultaneously
- **THEN** no two orbs appear identical in shape at any given moment due to staggered durations and directions

### Requirement: Orb Flow Motion Animation
Each orb SHALL animate its `transform` property using `@keyframes flow1` through `flow4`.

Each flow animation SHALL combine `translate`, `rotate`, and `scale` transformations to create orbital motion across the viewport.

Flow animations SHALL use durations of 22s, 25s, 26s, and 28s respectively, with `linear` timing.

#### Scenario: Orbs move across viewport
- **WHEN** the application is running
- **THEN** each orb slowly translates, rotates, and scales across the screen in a unique orbital pattern

### Requirement: Theme-Aware Orb Parameters
The `opacity`, `filter: blur()`, and `mix-blend-mode` of `.glow-orb` elements SHALL vary by active theme via CSS custom properties.

**google-glow**: opacity 0.65, blur 120px, mix-blend-mode screen
**obsidian**: opacity 0.45, blur 140px, mix-blend-mode screen
**frosted**: opacity 0.35, blur 140px, mix-blend-mode multiply

#### Scenario: Switch from google-glow to obsidian
- **WHEN** user changes theme from google-glow to obsidian
- **THEN** orb opacity decreases from 0.65 to 0.45, blur increases from 120px to 140px

#### Scenario: Switch to frosted light theme
- **WHEN** user changes theme to frosted
- **THEN** orb opacity drops to 0.35 and blend mode switches from screen to multiply

### Requirement: Orb Performance Optimization
The `.glow-orb` class SHALL include `will-change: transform, border-radius` for GPU pre-compositing.

The orb container SHALL have `overflow: hidden` to limit paint areas.

#### Scenario: Smooth animation on mid-range GPU
- **WHEN** application runs on integrated graphics (e.g., Intel UHD)
- **THEN** orb animations maintain at least 30fps without visible jank

### Requirement: Background Base Color
The background container SHALL set a solid base color behind the orbs that varies by theme.

**google-glow**: `#080808`
**obsidian**: `#030303`
**frosted**: `#f8fafc`

#### Scenario: Background base color renders
- **WHEN** no orbs are visible at a corner of the viewport
- **THEN** the solid base color fills the area according to the active theme
