# liquid-glass-ui

## Purpose

定义 Liquid Glass v3 设计系统要求，包括玻璃拟态视觉风格（v3 升级）、动态光球背景、暗色/浅色双模主题、排版、响应式布局、Framer Motion 动画集成和交互动效。

## Requirements

### Requirement: Liquid Glass Design Language
The interface SHALL implement Liquid Glass v3 design system, featuring:

- **Dynamic orb background**: 4 Google-colored orbs with CSS morphing and orbital animations behind all UI
- **Enhanced glass panels**: `backdrop-filter: blur(25-35px)` with saturation boost per theme, SVG `feTurbulence` noise texture overlay, asymmetric border highlights (top and left borders brighter than bottom/right)
- **Multi-layer shadows**: Combined outer shadow for depth (`0 10-20px 40-50px`) and inner highlight shadow (`inset 0 1-2px`) for glass edge refraction
- **Framer Motion-driven interactions**: Spring-based animations for panels, hover micro-interactions with `translateY(-2px)` lift effect

#### Scenario: Glass panel renders with v3 effects
- **WHEN** application window displays
- **THEN** all panel surfaces render with 25-35px backdrop blur, SVG noise texture grain, asymmetric border highlights, and multi-layer shadows

#### Scenario: Depth and layering
- **WHEN** glass panels at different z-levels overlap (background orbs → main window → sidebar → terminal → toolbar)
- **THEN** each layer shows accumulated blur and darkening effects through the translucent glass material, creating visual depth

#### Scenario: Hover and active states
- **WHEN** user hovers or interacts with UI elements (buttons, inputs, session items)
- **THEN** elements display smooth transitions in background opacity, border brightness, subtle `translateY(-2px)` lift, and glow effects over 0.3s cubic-bezier

### Requirement: Dark Theme Foundation
The interface SHALL use `google-glow` as the default dark theme with deep background and Google 4-color ambient orbs.

The default dark background SHALL be `#080808` with 4 colored orbs (blue, red, yellow, green) animating behind a glass application shell.

The `obsidian` theme SHALL provide an alternative darker aesthetic with near-black background (`#030303`), reduced orb opacity for neon contrast, and smoked dark glass fill.

#### Scenario: Default dark background with orbs
- **WHEN** application renders with google-glow theme
- **THEN** the root background is `#080808` with 4 morphing colored orbs providing ambient illumination behind the glass UI

#### Scenario: Text contrast on dark glass
- **WHEN** text displays on glass panels in dark themes
- **THEN** primary text uses bright white-toned colors, secondary text uses reduced opacity whites, ensuring readability against the dark glass background

### Requirement: Light Theme Foundation
The interface SHALL provide a `frosted` light theme with bright white-gray background (`#f8fafc`) and frosted white glass panels.

In light mode, orbs SHALL use `mix-blend-mode: multiply` with reduced opacity (0.35) to create watercolor-like diffusion on the light background.

Text colors in light mode SHALL invert to dark grays: primary `#1e293b`, secondary `#475569`, muted `#94a3b8`.

#### Scenario: Light theme background
- **WHEN** frosted theme is active
- **THEN** background is `#f8fafc` with pastel-toned orbs using multiply blend, glass panels are bright white-translucent with soft gray shadows

#### Scenario: Text readability in light mode
- **WHEN** text displays on glass panels in frosted theme
- **THEN** primary text is dark gray `#1e293b`, ensuring strong contrast against the bright glass background

### Requirement: Accent Colors and Gradients
The system SHALL use blue (`#4285F4`) as the primary accent color across all themes for consistency with the Google color system.

Primary action buttons SHALL use a blue-to-indigo gradient: `linear-gradient(135deg, #4285F4, #6366f1)` in dark themes, or solid blue in the frosted light theme.

Focus states SHALL show a blue outer glow appropriate to the active theme (stronger in dark, subtler in light).

#### Scenario: Primary button uses blue gradient
- **WHEN** the Send button renders in a dark theme
- **THEN** it displays a blue-to-indigo gradient background with a blue glow shadow

#### Scenario: Focus indicator
- **WHEN** an input element receives focus
- **THEN** it displays a blue glow ring appropriate to the active theme

#### Scenario: Progress indication
- **WHEN** a progress bar displays (file transfer)
- **THEN** the filled portion uses a gradient from blue through cyan to green, with a glow shadow

### Requirement: Terminal Typography
The terminal viewport SHALL use optimized monospace fonts for readability and UI chrome SHALL use clean sans-serif fonts. Both SHALL support Chinese character rendering.

#### Scenario: Terminal font
- **WHEN** terminal renders text
- **THEN** JetBrains Mono (fallback: Cascadia Code, Fira Code, system monospace) is used at comfortable reading size

#### Scenario: UI font
- **WHEN** UI chrome elements render text
- **THEN** Inter (fallback: system sans-serif) is used with appropriate weight and size hierarchy. Chinese characters fall back to system default Chinese fonts.

### Requirement: Responsive Layout with Resizable Panels
The application layout SHALL adapt to window size changes and allow user resizing of sidebar and file transmission panel.

#### Scenario: Window resize
- **WHEN** user resizes the application window
- **THEN** the terminal viewport fills available space, sidebar stays within min/max constraints, all glass panels correctly re-render blur at new dimensions

#### Scenario: Resizable sidebar
- **WHEN** user drags the divider between sidebar and terminal
- **THEN** sidebar width smoothly adjusts between 180px (min) and 400px (max)

#### Scenario: Resizable file transfer panel
- **WHEN** user drags the divider between file transfer panel and terminal
- **THEN** panel width smoothly adjusts between 160px (min) and 500px (max)

### Requirement: Smooth Animation and Transitions
The interface SHALL use Framer Motion and CSS transitions for all state changes and interaction feedback.

Hover micro-interactions SHALL include: background brightness change, border glow, `translateY(-2px)` lift, and shadow enhancement over 0.3s cubic-bezier.

The background orbs SHALL animate continuously via CSS `@keyframes` with durations between 12s and 28s.

#### Scenario: Panel slide animation
- **WHEN** side panel opens or closes
- **THEN** it transitions with Framer Motion spring animation without layout jumps or flickering

#### Scenario: Hover micro-interaction
- **WHEN** user hovers over interactive elements (buttons, session items)
- **THEN** elements lift by 2px, background brightens, and border glows over 0.3s

#### Scenario: Connection state transition
- **WHEN** connection state changes
- **THEN** status indicator plays a breathing pulse animation with glow effects

### Requirement: Framer Motion Integration
The system SHALL integrate Framer Motion library to drive all UI interaction animations.

#### Scenario: Component enter/exit
- **WHEN** components mount or unmount (tab switches, panel toggles)
- **THEN** AnimatePresence plays enter and exit animations

#### Scenario: Gesture interactions
- **WHEN** user performs complex gesture interactions (e.g., drag to resize panels)
- **THEN** visual feedback includes damping feel (resize handle glow line fade in/out)

### Requirement: Orb Background Integration in Layout
The `App` component SHALL render `GoogleGlowBackground` as the first child element, before the main application shell.

The main application shell SHALL be positioned at `z-index: 10` relative to the orb background at `z-index: 0`.

#### Scenario: Orb background renders behind app shell
- **WHEN** application displays
- **THEN** animated orbs are visible behind the glass application window, shining through translucent areas

### Requirement: Transparent Terminal Background
The XTerm.js terminal instance background SHALL be set to `transparent` to allow the orb background and glass effects to show through.

The terminal text SHALL use high-contrast colors appropriate for the active theme.

#### Scenario: Terminal transparency in dark theme
- **WHEN** a terminal session is active in google-glow or obsidian theme
- **THEN** the terminal background is transparent, showing the orb colors and glass texture behind the text

#### Scenario: Terminal readability in light theme
- **WHEN** a terminal session is active in frosted theme
- **THEN** terminal text is dark slate for readability against the bright glass background
