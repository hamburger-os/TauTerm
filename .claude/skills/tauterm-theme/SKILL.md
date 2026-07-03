---
name: tauterm-theme
description: >
  Enforce TauTerm's Liquid Glass v3 theme system when writing UI code. Use this skill whenever the user asks to create, modify, or style any React component, CSS Module, or inline style in the TauTerm project — including new dialogs, panels, buttons, inputs, sidebars, toolbars, status bars, or any visual element. Also use when the user asks about theme tokens, CSS variables, dark/light mode, or wants to ensure a component looks correct across themes. This skill ensures zero hardcoded colors and full theme compatibility across google-glow, obsidian, and frosted themes. Covers global utility classes (liquid-glass, liquid-primary-button, liquid-glass-button, liquid-glass-input), v3 asymmetric border highlights, SVG noise texture with per-theme opacity, accent-holofoil-gradient for primary actions, and color-mix() pattern for status-tinted backgrounds.
license: MIT
metadata:
  author: tauterm
  version: "2.0"
---

# TauTerm Liquid Glass v3 Theme System

Always follow these rules when writing any UI code in this project. The project uses CSS Modules + CSS Custom Properties — NO Tailwind CSS.

## Core Rule: Zero Hardcoded Colors

Every `color`, `background`, `border-color`, `box-shadow`, and `backdrop-filter` value MUST come from a CSS custom property defined in `src/styles/tokens.css`. The only exceptions are:

- `color: #fff` on `.liquid-primary-button` / `.primary` buttons (always white text on holographic gradient)
- Google orb colors (`#4285F4`, `#EA4335`, `#FBBC05`, `#34A853`) — these are a fixed design feature in `GoogleGlowBackground`
- Status colors MUST use tokens (`var(--color-success)`, `var(--color-warning)`, `var(--color-error)`, `var(--color-info)`) — do NOT hardcode `#34d399` / `#eab308`

## Token Reference

Full reference at `docs/theme-guide.md`. Quick lookup:

### Shared Constants (Level 1 — never change per theme)
```css
font-family: var(--font-ui);       /* Inter, sans-serif */
font-family: var(--font-mono);     /* JetBrains Mono, monospace */
font-size: var(--text-xs);         /* 0.7rem (~11.2px) */
font-size: var(--text-sm);         /* 0.78rem (~12px) */
font-size: var(--text-base);       /* 0.85rem (~14px) */
font-size: var(--text-md);         /* 0.95rem (~15px) */
font-size: var(--text-lg);         /* 1.1rem (~18px) */
font-size: var(--text-xl);         /* 1.25rem (20px) */
border-radius: var(--radius-xs);   /*  4px — micro: scrollbars, shortcut hints */
border-radius: var(--radius-sm);   /* 12px — control (compact): toolbar buttons, nav items, error boxes */
border-radius: var(--radius-md);   /* 12px — control (standard): buttons, inputs, selects, list items */
border-radius: var(--radius-lg);   /* 16px — panel: terminal modules, cards, glass panels */
border-radius: var(--radius-xl);   /* 24px — frame: dialogs, modals, main containers */
border-radius: var(--radius-2xl);  /* 24px — frame alias (same as xl, backward compatible) */
border-radius: var(--radius-full); /* 9999px — pill: toggles, badges, status dots */
padding: var(--spacing-md);        /* 12px (xs:4 sm:8 md:12 lg:16 xl:24 2xl:32) */
transition: all var(--transition-fast);  /* 150ms ease */
transition: all var(--transition-normal); /* 300ms ease */
transition: all var(--transition-button); /* 0.3s cubic-bezier(0.4, 0, 0.2, 1) — button hover lift */
transition: all var(--transition-input);  /* 0.3s ease — input focus glow */
transition: all var(--transition-knob);   /* 200ms ease — toggle knob position */
z-index: var(--z-sidebar);         /* 10 */
z-index: var(--z-panel);           /* 20 */
z-index: var(--z-overlay);         /* 30 */
z-index: var(--z-toast);           /* 50 */
blur: var(--blur-xs);              /*  4px — overlay backdrops (shared constant) */
```

### Border-Radius Tier Quick Reference (v3.1)

| Semantic Tier | Value | Token(s) | Use For |
|--------------|-------|----------|---------|
| Frame | 24px | `--radius-xl`, `--radius-2xl` | Dialogs, modals, settings container, command palette |
| Panel | 16px | `--radius-lg` | Terminal viewport, layout chrome (toolbar/sidebar/statusbar/sendbar/transmission panel), cards, glass panels |
| Control | 12px | `--radius-sm`, `--radius-md` | Buttons, inputs, selects, list/nav items |
| Window | 8px | `--radius-window` | Window-level chrome corners (toolbar top / statusbar bottom), matches OS frame curvature |
| Pill | 9999px | `--radius-full` | Toggle tracks/knobs, badges, indicator dots |
| Micro | 4px | `--radius-xs` | Scrollbar thumbs, tiny shortcut badges |

**Edge-contact (0px) exception**: Only when an element's outer edge touches a screen edge or parent boundary (e.g., fullscreen terminal viewport, dividers). Rounding edge-touching elements creates awkward gaps. Layout chrome surfaces use `app-root` `gap: 6px` / `padding: 6px` for breathing room; flush inner edges get clipped by `overflow: hidden` → effectively 0px.

### Theme Tokens (Level 2 — vary per theme)

**Text:**
```css
color: var(--text-primary);     /* main text — body, active items, headings, file names */
color: var(--text-secondary);   /* secondary text — form labels, setting labels, button labels,
                                   status text, port info, section headers, descriptions.
                                   Use for ANY text the user needs to read to operate the UI. */
color: var(--text-muted);       /* dim text — RESERVED for: placeholders, timestamps, keyboard
                                   shortcuts, file-size metadata, version numbers, disabled states.
                                   NEVER use for labels, identifiers, or descriptions that users
                                   need to read. When in doubt, choose --text-secondary. */
```

**Text Color Decision Rule (choose `--text-muted` vs `--text-secondary`):**
- Can the user still operate this part of the UI **without** reading this text? → If YES, `--text-muted` is OK (placeholder, timestamp, shortcut, version)
- Is this text a label, section header, field name, port/endpoint identifier, or description that helps the user understand what to do? → `--text-secondary`
- If unsure → `--text-secondary`. Text that is slightly too bright is always better than text that is unreadable.

**Accent:**
```css
color: var(--accent-primary);
color: var(--accent-secondary);
background: var(--accent-gradient);  /* blue→indigo */
box-shadow: 0 0 10px var(--accent-glow);
color: var(--text-on-accent);       /* Always #fff — text on accent backgrounds */
```

**Blur (theme-dependent — values vary per theme):**
```css
backdrop-filter: blur(var(--blur-light));  /* 8px (all themes share 8px) */
backdrop-filter: blur(var(--blur-medium)); /* 16px google-glow / 20px obsidian + frosted */
backdrop-filter: blur(var(--blur-heavy));  /* 24px google-glow / 30px obsidian / 35px frosted */
backdrop-filter: blur(var(--glass-blur));  /* 25px google-glow / 30px obsidian / 35px frosted — v3 primary panel blur */
```

**Glass panels:**
```css
background: var(--glass-fill);         /* glass fill gradient */
backdrop-filter: blur(var(--glass-blur)) saturate(var(--glass-blur-saturate));
border: 1px solid var(--glass-border-default);  /* glass border */
border-top: 1px solid var(--glass-border-top);
border-left: 1px solid var(--glass-border-left);
box-shadow: var(--glass-shadow-outer), var(--glass-shadow-inner);
/* Terminal text shadow (frosted theme uses this for readability on glass) */
text-shadow: var(--glass-shadow-text);
/* Active/hover states */
background: var(--glass-bg-active);
border-color: var(--glass-border-focus);
```

**Glass buttons:**
```css
background: var(--glass-button-bg);
border: 1px solid var(--glass-button-border);
box-shadow: var(--glass-button-shadow-inset);
/* hover: */
background: var(--glass-button-hover-bg);
border-color: var(--glass-button-hover-border);
```

**Glass inputs:**
```css
background: var(--glass-input-bg);
border: 1px solid var(--glass-input-border);
box-shadow: var(--glass-input-shadow-inner);
/* focus: */
border-color: var(--glass-input-focus-border);
box-shadow: var(--glass-input-shadow-inner), var(--glass-input-focus-glow);
```

**Primary action button (holographic):**
```css
background: var(--accent-holofoil-gradient);  /* multi-color holographic */
background-size: 300% 300%;
animation: gradient-shift 4s ease infinite;
border: 1px solid rgba(255, 255, 255, 0.5);
box-shadow: inset 0 1px 2px rgba(255, 255, 255, 0.8), 0 4px 15px rgba(168, 85, 247, 0.3);
color: #fff;
/* Use .liquid-primary-button global class or .primary GlassButton variant */
```

**Dialogs & Overlays:**
```css
background: var(--overlay-bg);       /* modal backdrop */
background: var(--dialog-bg);        /* dialog surface */
box-shadow: var(--shadow-glass), var(--dialog-shadow);
box-shadow: var(--shadow-elevated);  /* floating elements (Toast) */
```

**Select dropdowns:**
```css
/* option element */
background: var(--select-option-bg);
/* Note: select arrow fill color uses var(--select-arrow) token defined per theme */
```

**Status colors:**
```css
color: var(--color-success);   /* green */
color: var(--color-error);     /* red */
color: var(--color-warning);   /* amber */
color: var(--color-info);      /* blue */
/* For tinted backgrounds, use color-mix() — never hardcode rgba: */
background: color-mix(in srgb, var(--color-error) 15%, transparent);
```

**Tool panel helper tokens (RightSidebar + Tools):**
```css
/* These are defined per-theme in tokens.css and used by
   RightSidebarPanel, CalculatorTool, ChecksumTool, EncodingTool,
   BitOpsTool, and ProtocolTool components. They exist alongside
   the main glass tokens to provide finer control over tool panel
   appearance without coupling to layout chrome values. */
color: var(--text-tertiary);           /* dim hint/placeholder text in tools */
background: var(--glass-hover);        /* panel header hover state */
background: var(--glass-fill-secondary);  /* tool result cards, mode button bg */
background: var(--color-accent);       /* active tab/mode button — aliases accent-primary */

/* --color-accent is a semantic alias for var(--accent-primary).
   In tool panels it serves as the active-state fill for mode buttons
   and tab selectors. Always paired with var(--text-on-accent) for
   accessible contrast on the filled background. */

/* --glass-hover and --glass-fill-secondary are subtle semi-transparent
   overlays tuned per theme:
   - google-glow: white 8% / 3%
   - obsidian:     white 6% / 2%
   - frosted:      black 5% / 2%
   These replace ad-hoc rgba() values and automatically invert for
   light vs dark backgrounds. */
```

**Background orbs (GoogleGlowBackground):**
```css
opacity: var(--bg-orb-opacity);
filter: blur(var(--bg-orb-blur));
mix-blend-mode: var(--bg-orb-blend);
```

## Global CSS Utility Classes

These global classes from `src/styles/global.css` can be used alongside CSS Modules:

| Class | When to use |
|-------|-------------|
| `.liquid-glass` | Full glass panel: SVG noise texture (via ::before with per-theme opacity) + asymmetric top/left highlight borders + multi-layer shadows + saturate filter. **Use on ALL layout chrome surfaces** (toolbar, sidebar, statusbar, terminal viewport, sendbar, transmission panel) **and** dialogs/popups/dropdowns. Used by `GlassPanel` component automatically. |
| `.liquid-glass-card` | Glass card: inherits `glass-fill` fill + asymmetric highlight borders, uses `shadow-elevated` (16px) shadow. **Use for ALL inner cards nested inside a `.liquid-glass` surface** (aggregate progress card, mode selection cards, settings panel cards, stats dashboard cards, etc.). No `backdrop-filter` (parent Surface already provides blur), no `::before` noise, no `position: relative` constraint — CAN be used on `position: absolute/fixed` elements. For very small list rows (~30-50px), use the Mini-Card pattern instead (see Card Patterns section). |
| `.liquid-primary-button` | Primary action button: holographic gradient with `gradient-shift` animation, glass blur, white text. Use for Send, Connect, Submit buttons. |
| `.liquid-glass-button` | Secondary glass button: semi-transparent bg, hover lift effect. Use for Cancel, Close, auxiliary actions, icon buttons, option selectors. |
| `.liquid-glass-input` | Glass input: dark inset bg, accent glow on focus. Use for search bars, text inputs, textareas, selects. |
| `.glow-orb` | Animated background orb. Used internally by `GoogleGlowBackground`. |

```tsx
// [正确] Combine CSS Module + global utility class
<button className={`${styles.sendBtn} liquid-primary-button`}>Send</button>
<input className={`${styles.search} liquid-glass-input`} />
<div className={`${styles.card} liquid-glass`}>Content</div>
```

**警告： `.liquid-glass` positioning constraint**: `.liquid-glass` forces `position: relative` (needed for the `::before` noise texture). Do NOT use `.liquid-glass` on elements that need `position: absolute` or `position: fixed` (context menus, floating search bars, toast notifications, absolutely-positioned dropdowns). These elements must keep their glass properties (`background`, `border`, `box-shadow`, `backdrop-filter`) in their own CSS Module.

**Floating element glass pattern** (for `position: absolute` / `position: fixed` elements that can't use `.liquid-glass`):

```css
/* [正确] Correct — v3 tokens, self-contained glass */
.floatingPanel {
  position: absolute; /* or fixed */
  background: var(--dialog-bg);
  backdrop-filter: blur(var(--glass-blur));
  -webkit-backdrop-filter: blur(var(--glass-blur));
  border: 1px solid var(--glass-border-default);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-glass), var(--dialog-shadow);
}

/* [错误] Wrong — using incorrect tokens on floating element */
.floatingPanel {
  backdrop-filter: blur(var(--blur-medium));   /* use --glass-blur */
  border: 1px solid var(--glass-border-default); /* use glass token */
}
```

**Default border-radius**: `.liquid-glass-button` and `.liquid-glass-input` provide `border-radius: var(--radius-md)` (12px, Control tier) by default. CSS Modules may override with a different value (e.g., `--radius-sm`, `--radius-lg`) — the module CSS loads after global CSS and automatically wins the cascade.

## Component Patterns

### CSS Module Component

```css
/* [正确] Correct — v3 primary tokens */
.myComponent {
  background: var(--glass-fill);
  border: 1px solid var(--glass-border-default);
  border-radius: var(--radius-md);  /* 12px — Control tier */
  color: var(--text-primary);
  padding: var(--spacing-md);
}
.myComponent:hover {
  background: var(--glass-bg-hover);
  border-color: var(--glass-border-hover);
}

/* [错误] Wrong */
.myComponent {
  background: rgba(0, 0, 0, 0.2);   /* hardcoded, won't work in frosted theme */
  border: 1px solid #fff;            /* invisible on light background */
  color: #888;                       /* fixed, won't change with theme */
  border-radius: 4px;                /* too small for glass feel — use Control tier (12px) */
}
```

### Layout Chrome Pattern (v1.6)

Layout chrome surfaces (toolbar, sidebar, status bar, terminal viewport, send bar, transmission panel, right sidebar) MUST use the `.liquid-glass` global class. CSS Modules for layout chrome ONLY contain layout properties + `border-radius: var(--radius-lg)` (16px, Panel tier) — NEVER visual glass properties. The `app-root` provides `gap: 6px` and `padding: 6px` to create breathing room for rounded corners. Inner edges flush against adjacent surfaces get clipped by `overflow: hidden`, effectively applying the 0px edge-contact rule:

```css
/* [正确] Correct — CSS Module: layout + border-radius only */
.toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  height: 36px;
  padding: 0 var(--spacing-md);
  flex-shrink: 0;
  user-select: none;
  border-radius: var(--radius-lg);  /* 16px — Panel tier */
  /* NO background, border, box-shadow, or backdrop-filter here */
}

/* [错误] Wrong — CSS Module hand-rolling glass */
.toolbar {
  background: var(--block-toolbar-bg);              /* @deprecated — use .liquid-glass */
  backdrop-filter: blur(var(--blur-medium));         /* @deprecated — use .liquid-glass */
  border-bottom: 1px solid var(--glass-border-default); /* @deprecated — use .liquid-glass */
}
```

```tsx
// JSX: compose CSS Module class + liquid-glass global class
<div className={`${styles.toolbar} liquid-glass`}>
```

### Glass Surface Tier Selection (v3.2)

TauTerm has two glass surface tiers. Choose based on whether the element is a top-level layout surface or a nested inner card:

| Tier | Class | Shadow | Backdrop Blur | Noise | Position | Use For |
|------|-------|--------|---------------|-------|----------|---------|
| **Surface** | `.liquid-glass` | `--glass-shadow-outer` + `--glass-shadow-inner` (40px) | blur(25-35px) | YES | `relative` | Layout chrome (sidebar, toolbar, terminal, statusbar, sendbar, transmission panel), dialogs, modals |
| **Card** | `.liquid-glass-card` | `--shadow-elevated` (16px) | NO | NO | `static` | Inner cards ≥50px height, nested inside a `.liquid-glass` surface (mode selection cards, settings cards, stats dashboard cards, aggregate progress cards) |

**Selection rules**:
- Element faces the page background or forms an independent visual region → `.liquid-glass`
- Element is nested inside another `.liquid-glass` surface → `.liquid-glass-card`
- **NEVER nest `.liquid-glass` inside `.liquid-glass`** — causes 40px shadow stacking + backdrop-filter compounding + triple noise overlay
- `.liquid-glass-card` is safe for `position: absolute` / `position: fixed` elements (no `position: relative` constraint)

```tsx
// [正确] Correct — outer Surface + inner Card
<div className={`${styles.panel} liquid-glass`}>
  <div className={`${styles.card} liquid-glass-card`}>
    {/* inner card content */}
  </div>
</div>

// [错误] Wrong — nested .liquid-glass inside .liquid-glass
<div className={`${styles.panel} liquid-glass`}>
  <div className={`${styles.card} liquid-glass`}>
    {/* 40px shadow inside an already-blurred panel — visually heavy */}
  </div>
</div>
```

### Mini-Card CSS Module Pattern (v3.3)

Very small elements (~30-50px) nested inside `.liquid-glass` surfaces should NOT use `.liquid-glass-card` (16px shadow is too heavy). Instead, use module-specific CSS with the Mini-Card pattern:

```css
/* Mini-Card — small rows/labels inside .liquid-glass surfaces
   Uses glass-fill + 3D asymmetric borders + lightest shadow (6px) */
.miniCard {
  background: var(--glass-fill);
  border: 1px solid var(--glass-border-default);
  border-top: 1px solid var(--glass-border-top);    /* 3D highlight — brighter top */
  border-left: 1px solid var(--glass-border-left);  /* 3D highlight — mid-bright left */
  border-radius: var(--radius-sm);                   /* 12px — Control tier */
  box-shadow: var(--shadow-sm);                      /* 6px — lighter than card tier */
}
```

Elements that should use this pattern:
- Transmission panel file summary bars (`.fileSummary`)
- PerFileList file rows (`.row`) — ~35px, consistent with other small elements in the panel
- Other non-interactive info bars/labels under ~50px height

The 3D asymmetric borders maintain visual language consistency with larger `.liquid-glass-card` elements while `shadow-sm` provides appropriate depth for the element's size.
```

### Layout Bar Alignment (v1.8)

All layout chrome bars (toolbar, sidebar, status bar, send bar, transmission panel) MUST follow these alignment rules:

- Use `display: flex; align-items: center;` — children are **vertically centered**, NEVER `flex-end` or `flex-start`
- **Fixed-height bars** use a fixed `height` (Toolbar=36px, StatusBar=26px), NOT `min-height`
- **Flex panels** (sidebar, transmission panel, sendbar) use `height: 100%` / `flex: 1` to fill the parent; sendbar additionally sets `min-height: var(--sendbar-min-height)` and is user-resizable via vertical `ResizeHandle`
- Buttons/inputs/selects inside the bar use consistent vertical `padding` (e.g., `4px 8px`) so all controls share the same height and text baselines align horizontally
- Child groups (e.g., `.actions`) also use `align-items: center` internally

```css
/* [正确] Correct — vertical center + fixed height (toolbar/statusbar) */
.toolbar {
  display: flex;
  align-items: center;
  height: 36px;
  padding: 0 var(--spacing-md);
}
.statusBar {
  display: flex;
  align-items: center;
  height: 26px;
}

/* [正确] Correct — flex panel (sendbar): flex ratio + min-height, no fixed height */
.sendBar {
  display: flex;
  align-items: center;
  flex: 1;
  min-height: var(--sendbar-min-height);
  gap: 6px;
  padding: 6px 8px;
}

/* [错误] Wrong — flex-end causes controls to sit at different vertical positions */
.sendBar {
  align-items: flex-end;   /* jagged bottom alignment */
}
```

> **Convention**: Toolbar=36px, StatusBar=26px (fixed height). Panels (sidebar, right sidebar, transmission panel, sendbar) use `height: 100%` / `flex: 1` to fill the parent. Sendbar additionally enforces `min-height: var(--sendbar-min-height)` and is user-resizable via vertical `ResizeHandle` drag.

### RightSidebar Panel Pattern (v3.5)

The right sidebar uses a collapsible accordion panel system (`RightSidebarPanel`). Each panel wraps a tool or sub-component and provides a toggleable header with chevron rotation and CSS `max-height` transition animation.

**Container (`RightSidebar`):**
```tsx
// Wraps child panels with liquid-glass styling
<aside className={`${styles.sidebar} liquid-glass`} style={{ width }} aria-label="Right sidebar">
  <div className={styles.scrollArea}>
    {children}  {/* RightSidebarPanel components */}
  </div>
</aside>
```

**Accordion Panel (`RightSidebarPanel`):**
```css
/* Panel separator — matches glass border color */
.panel {
  border-bottom: 1px solid var(--glass-border-default);
  overflow: hidden;
}
.panel:last-child {
  border-bottom: none;
}
/* Header button — hover uses glass-hover token */
.header {
  padding: 6px var(--spacing-sm);
  background: none;
  border: none;
  color: var(--text-primary);
  font-size: var(--text-sm);
  transition: background-color 0.15s ease;
}
.header:hover {
  background-color: var(--glass-hover);
}
/* Chevron rotation animation */
.chevron {
  transition: transform 0.2s ease;
}
.chevronOpen {
  transform: rotate(-180deg);
}
/* Body collapse animation via max-height */
.body {
  overflow: hidden;
  transition: max-height 0.25s ease, opacity 0.2s ease;
}
```

**Key rules for RightSidebar panels:**
- `.scrollArea` MUST include `padding: 4px 0` and `gap: 2px` for visual separation from glass borders and between panels
- Panel headers MUST use `aria-expanded={expanded}` on the toggle button
- Content height is measured via `ResizeObserver` for dynamic max-height transitions — ensure the observer is disconnected in cleanup
- Inner tool content (`<div>` child inside `.body`) receives `padding: var(--spacing-sm)` for consistent internal spacing

### Embedded Tool Panel Pattern (v3.5)

Tools in the right sidebar (`ChecksumTool`, `EncodingTool`, `BitOpsTool`, `ProtocolTool`) each follow a consistent pattern:

```tsx
// Inner component — used standalone or embedded in CalculatorTool tabs
export function ToolNameInner() {
  const { t } = useTranslation();
  // ... state, useMemo for computation, handlers
  return (
    <div className={styles.container}>
      {/* tool-specific UI — inputs, selects, results */}
    </div>
  );
}

// Default export — wraps Inner in RightSidebarPanel for standalone use
export default function ToolName() {
  const { t } = useTranslation();
  return (
    <RightSidebarPanel title={t("tools.toolName")}>
      <ToolNameInner />
    </RightSidebarPanel>
  );
}
```

**Key rules for embedded tools:**
- Input elements (textarea, input, select) MUST use `liquid-glass-input` global class
- Result displays use `var(--color-success)` for valid results, `var(--color-error)` for error states
- Error result strings MUST use the `[Error: ...]` prefix convention for detection and red styling
- Monospace values use `var(--font-mono)` with `font-size: var(--text-xs)`
- All user-facing labels, placeholders, and button text MUST use `t()` i18n — never hardcode Chinese in parse logic

#### `--sendbar-min-height` Token (v3.2)

This is a **layout constant** defined in `src/styles/tokens.css` (Level 1 — shared across all themes). JS code that needs to know the SendBar minimum height (e.g., for drag-resize calculations in `App.tsx`) MUST read it dynamically via `getComputedStyle` rather than hardcoding a numeric value:

```tsx
// [正确] Correct — read from CSS, single source of truth
useEffect(() => {
  const val = getComputedStyle(document.documentElement)
    .getPropertyValue("--sendbar-min-height").trim();
  const parsed = parseInt(val, 10);
  if (!isNaN(parsed)) setSendbarMinHeight(parsed);
}, []);

// [错误] Wrong — hardcoded 90px/106px will drift from CSS
const minHeight = 90;
```

### Font-Size Token Guidance (v1.8)

All `font-size` values ≥ ~11px MUST use `--text-*` tokens. Only micro-text (8px/9px/10px) for status bars, badges, and tiny labels may use raw px values:

| Token | rem | ≈px | Use For |
|-------|-----|-----|---------|
| `--text-xs` | 0.7rem | ≈11px | Auxiliary labels, search fields, toolbar buttons, small controls |
| `--text-sm` | 0.78rem | ≈12px | Secondary text, list item names, form labels |
| `--text-base` | 0.85rem | ≈13px | Body text, nav items, settings options, descriptions |
| `--text-md` | 0.95rem | ≈15px | Titles, icon buttons, larger controls |
| `--text-lg` | 1.1rem | ≈18px | Panel titles, dialog titles |
| `--text-xl` | 1.25rem | ≈20px | Large headings (rare) |

```css
/* [正确] Correct — tokens for 11px+ */
font-size: var(--text-sm);       /* 12px → token */
font-size: var(--text-base);     /* 13px → token */
font-size: var(--text-xs);       /* 11px → token */

/* [正确] Correct — px for micro-text (8/9/10px only) */
font-size: 10px;                 /* status bar info — micro tier */
font-size: 9px;                  /* section labels — micro tier */
font-size: 8px;                  /* tiny badges — micro tier */

/* [错误] Wrong — raw px where a token exists */
font-size: 14px;                 /* use var(--text-md) */
font-size: 13px;                 /* use var(--text-base) */
font-size: 12px;                 /* use var(--text-sm) */
font-size: 11px;                 /* use var(--text-xs) */
```

### Inline Style Component

```tsx
// [正确] Correct
<div style={{
  backgroundColor: "var(--bg-base)",
  color: "var(--text-secondary)",
  border: "1px solid var(--glass-border-default)",
  borderRadius: "var(--radius-md)",
}} />

// [错误] Wrong
<div style={{
  backgroundColor: "#0a0a1a",        // hardcoded dark
  color: "#888",                      // won't change with theme
  border: "1px solid rgba(0,255,255,0.15)", // v2 teal legacy
}} />
```

### New Dialog/Popup Template

```css
.dialog {
  background: var(--dialog-bg);
  border: 1px solid var(--glass-border-default);
  border-radius: var(--radius-xl);  /* 24px — Frame tier */
  box-shadow: var(--shadow-glass), var(--dialog-shadow);
  backdrop-filter: blur(var(--glass-blur));
  -webkit-backdrop-filter: blur(var(--glass-blur));
}
/* Modal overlay */
.overlay {
  background: var(--overlay-bg);
  backdrop-filter: blur(var(--blur-xs));
  -webkit-backdrop-filter: blur(var(--blur-xs));
}
```

### New Select Dropdown Template (v3.6)

**Required: combine CSS Module + TWO global classes `.liquid-glass-input` and `.liquid-glass-select`**

Arrow styling, height, and padding are now provided by the global `.liquid-glass-select` class.
Tokens `--select-height` (28px) and `--select-padding` (2px 8px) ensure all selects are consistent across the app.

```css
/* CSS Module — layout-only: width, font-size, and any component-specific overrides */
.select {
  width: 100%;
  height: var(--select-height);
  padding: var(--select-padding);
  font-size: var(--text-xs);
  font-family: var(--font-ui);
  transition: all var(--transition-input);
}
```

```tsx
// JSX — `.liquid-glass-input` provides bg/border/color/shadow/focus/hover/disabled
//        `.liquid-glass-select` provides appearance:none, arrow, cursor, height, padding
<select className={`${styles.select} liquid-glass-input liquid-glass-select`}>
  <option value="a">A</option>
</select>
```

> `.liquid-glass-input` handles: `background-color`, `border`, `box-shadow`, `color`,
> `outline`, `:focus` glow, `:hover` border, `:disabled` transparency, `border-radius`.
> `.liquid-glass-select` handles: `appearance:none`, arrow SVG, `padding-right`, `cursor`,
> `height`, `padding`, `option` bg/color.
> CSS Modules only need to define: `width`/`flex`, component-specific overrides, and `transition`.

### Number Input Spin Button Pattern (v3.4)

Number inputs (`<input type="number">`) with the `liquid-glass-input` class get auto-styled spin buttons (▲▼ arrows) from `global.css`. These use an inline SVG `background-image` with `fill="currentColor"` for automatic theme adaptation. CSS Modules should ONLY define layout/sizing — never override spin button visuals.

**Global CSS handles (DO NOT redefine in CSS Modules):**
```css
/* global.css — these already exist, no need to copy */
.liquid-glass-input[type="number"]::-webkit-inner-spin-button {
  /* SVG arrows via background-image, color via var(--text-secondary) */
  /* hover: opacity 1, active: color → var(--text-primary) */
  /* disabled: opacity 0.3 */
}
```

**CSS Module — layout/sizing only:**
```css
/* [正确] Correct — layout only, spin button visuals are global */
.intervalInput {
  width: 52px;
  padding: 4px 6px;
  border-radius: var(--radius-md);   /* 12px — Control tier */
  font-size: var(--text-xs);
  font-family: var(--font-mono);
  text-align: right;
  /* NO background, NO background-image, NO ::-webkit-inner-spin-button overrides */
}

/* [错误] Wrong — overriding global spin button visuals */
.intervalInput::-webkit-inner-spin-button {
  background: red;   /* breaks theme consistency */
}
```

**JSX — compose with global class:**
```tsx
// [正确] Correct
<input type="number"
  className={`${styles.intervalInput} liquid-glass-input`}
  value={interval} onChange={...} min={50} step={100} />

// [错误] Wrong — missing liquid-glass-input → no spin button styling
<input type="number" className={styles.intervalInput} />

// [错误] Wrong — reusing .select class on number input
// .select applies var(--select-arrow) background-image → conflicts with spin button
<input type="number" className={`${styles.select} liquid-glass-input`} />
```

**Rules:**
- Number inputs MUST use `className={${styles.xxx} liquid-glass-input}` — the global class provides spin button styling
- CSS Modules MUST NOT define `::-webkit-inner-spin-button` / `::-webkit-outer-spin-button` pseudo-elements — global CSS owns these
- CSS Modules MUST NOT set `background-image` on number inputs — conflicts with the spin button SVG arrows
- Do NOT reuse the `.select` CSS Module class (which has `var(--select-arrow)` background-image) on `<input type="number">` — always use a dedicated number input class
- The spin button SVG uses `fill="currentColor"` — the color adapts via the pseudo-element's `color` property set to `var(--text-secondary)` in global CSS
- Spin button width is 22px (comfortable touch target) — ensure the number input is wide enough to accommodate (minimum ~50px for 2-3 digit values)

### Status-Tinted Background (color-mix pattern)

Use `color-mix()` to create theme-adaptive tinted backgrounds. This is the canonical pattern for error boxes, warning banners, success indicators, and signal badges:

```css
/* [正确] Correct — adapts to all 3 themes */
.errorBanner {
  background: color-mix(in srgb, var(--color-error) 15%, transparent);
  border: 1px solid color-mix(in srgb, var(--color-error) 30%, transparent);
  color: var(--color-error);
}
.warningBanner {
  background: color-mix(in srgb, var(--color-warning) 10%, transparent);
  border-left: 3px solid var(--color-warning);
}
.successBadge {
  background: color-mix(in srgb, var(--color-success) 12%, transparent);
}

/* [错误] Wrong — would need different rgba per theme */
.errorBanner {
  background: rgba(234, 67, 53, 0.15);  /* only works in google-glow */
}
```

### GlassButton Variants

The `GlassButton` component supports four variants:

| Variant | CSS Class | Visual Style |
|---------|-----------|-------------|
| `primary` | `.primary` | Holographic gradient + gradient-shift animation + white text |
| `secondary` | `.secondary` | Default glass button bg + border (also exposed as `.liquid-glass-button` global) |
| `ghost` | `.ghost` | Transparent bg, color on hover, no border |
| `danger` | `.danger` | Red-tinted bg via `color-mix(in srgb, var(--color-error) 10%, transparent)` |

```tsx
<GlassButton variant="primary">Connect</GlassButton>
<GlassButton variant="secondary">Cancel</GlassButton>
<GlassButton variant="ghost">Edit</GlassButton>
<GlassButton variant="danger">Delete Session</GlassButton>
```

### GlassPanel Options

```tsx
<GlassPanel variant="default" padding="md">...</GlassPanel>
<GlassPanel variant="elevated" padding="lg">...</GlassPanel>
```

| Prop | Values | Effect |
|------|--------|--------|
| `variant` | `"default"`, `"elevated"` | `elevated` adds stronger border highlight + enhanced shadow |
| `padding` | `"none"`, `"sm"`, `"md"`, `"lg"` | Maps to `--spacing-*` tokens |

### Toggle Switch Pattern

The canonical toggle switch pattern (from SendBar.module.css):

```css
/* Hidden native checkbox */
.toggleCheck {
  position: absolute;
  opacity: 0;
  width: 0; height: 0;
}
/* Custom track */
.toggleTrack {
  width: 30px; height: 17px;
  background: var(--glass-input-bg);
  border: 1px solid var(--glass-input-border);
  border-radius: var(--radius-full);
  box-shadow: var(--glass-input-shadow-inner);
  transition: all var(--transition-button);
}
/* Slider knob */
.toggleTrack::after {
  content: "";
  position: absolute;
  top: 2px; left: 2px;
  width: 11px; height: 11px;
  border-radius: var(--radius-full);  /* 9999px — Pill tier */
  background: var(--text-muted);
  transition: all var(--transition-button);
}
/* Checked: track fills with accent gradient */
.toggleCheck:checked + .toggleTrack {
  background: var(--accent-gradient);
  border-color: transparent;
  box-shadow: 0 0 10px var(--accent-glow);
}
/* Checked: knob slides right + turns white */
.toggleCheck:checked + .toggleTrack::after {
  left: 15px;
  background: var(--text-primary);
}
```

### Indicator Dot Pattern

State colors MUST be controlled via CSS Module variant classes — NOT inline `style` attributes. Use `.dotConnected` / `.dotDisconnected` class composition:

```css
.dot {
  width: 7px; height: 7px;
  border-radius: var(--radius-full);  /* 9999px — Pill tier */
  flex-shrink: 0;
}
.dotConnected {
  background: var(--color-success);
  box-shadow: 0 0 6px var(--color-success);
}
.dotDisconnected {
  background: var(--color-error);
  box-shadow: 0 0 6px var(--color-error);
}
```

```tsx
// Class composition switches state — avoid inline style
<span className={`${styles.dot} ${isConnected ? styles.dotConnected : styles.dotDisconnected}`} />
```

For pulsing connected indicators (session sidebar), add the `pulse` animation:

```css
.statusDot {
  width: 7px; height: 7px;
  border-radius: var(--radius-full);
  background: var(--text-muted);
}
.statusDot.connected {
  background: var(--color-success);
  box-shadow: 0 0 10px var(--color-success);
  animation: pulse 2s ease-in-out infinite;
}
```

### DualPane Component Pattern

DualPane is the dual-column ASCII/HEX display component for serial terminal data. It follows a strict separation: **static visual properties** live in CSS Module classes using tokens, **dynamic layout values** live in React inline styles.

**Token hard requirements:**

| Element | Must Use | Must NOT |
|---------|----------|----------|
| Container font | `var(--font-mono)` | Hardcoded font stack |
| TX row color | `var(--accent-secondary)` | Hardcoded blue/cyan |
| RX row color | `inherit` (inherits `--text-primary`) | Separate color token |
| Divider default | `var(--glass-border-default)` | Any other border color |
| Divider hover/active | `var(--glass-border-hover)` | Any other hover color |
| Timestamp label | `var(--text-secondary)` + `opacity: 0.6` | `--text-muted` or hardcoded |
| Scrollbar thumb | `var(--glass-border-default)` | Hardcoded color |

**Dynamic layout values (inline style only):**

```tsx
// Container: fontSize via inline style (user-adjustable from localStorage)
<div className={styles.container} style={{ fontSize: `${fontSize}px` }}>

// ASCII cell: width via inline style (draggable split, defaults 33.33%)
<div className={styles.asciiCell} style={{ width: `${splitPct}%` }}>

// Divider: left position via inline style (tracks the split)
<div className={styles.divider} style={{ left: `${splitPct}%` }}>
```

**Rules:**
- Never create CSS custom properties (e.g., `--dual-split`, `--dual-font-size`) — use inline styles directly
- Never hardcode font stacks — always use `var(--font-mono)`
- Never hardcode colors in DualPane — all colors must reference theme tokens
- Divider must use `position: absolute; transform: translateX(-50%)` — the inline `left` value is the split percentage
- Divider must carry full ARIA: `role="separator"`, `aria-orientation="vertical"`, `aria-valuenow`, `aria-valuemin/max`, `aria-label`, `tabIndex={0}`
- Dragging state is driven by `useState` → CSS class `dividerActive` toggle (not ref)

### Floating Overlay Buttons (v3.2)

Floating buttons that sit on top of scrollable content (e.g., "scroll to bottom") use a circular glass variant:

```css
/* ScrollToBottomButton.module.css */
.button {
  position: absolute;
  bottom: 16px;
  right: 28px;
  z-index: var(--z-overlay);
  width: 36px;
  height: 36px;
  border-radius: 50%;
  background: var(--glass-fill);
  backdrop-filter: blur(var(--glass-blur));
  -webkit-backdrop-filter: blur(var(--glass-blur));
  border: 1px solid var(--glass-border-default);
  box-shadow: var(--glass-shadow-outer), var(--glass-shadow-inner);
  display: flex;
  align-items: center;
  justify-content: center;
  cursor: pointer;
  color: var(--text-secondary);
  transition: all var(--transition-fast);
}
```

**Rules:**
- Use `position: absolute` inside a `position: relative` container (e.g., `.terminalInstanceWrapper`)
- Use `border-radius: 50%` for circular shape, `var(--glass-fill)` + `backdrop-filter` for glass effect
- Hover: `color: var(--text-primary)`, `border-color: var(--glass-border-hover)`, `background: var(--glass-bg-hover)`, `transform: translateY(-1px)`, `box-shadow: var(--shadow-elevated), var(--glass-shadow-inner)`
- Parent container MUST have `position: relative` (added via a wrapper `<div>` if the terminal element itself is not positioned)
- Use `React.memo` to avoid re-renders from parent data flow (e.g., terminal data streaming)
- Use `AnimatePresence` from framer-motion for enter/exit animations
- `aria-label` must use i18n key (e.g., `t("terminal.scrollToBottom")`)
- This button belongs to Category A (absolute/fixed positioned elements) for theme-review — the CSS Module must inline glass styles, NOT use global Liquid Glass classes

### Settings Panel Recording/Conflict Animations (v3.2)

The ShortcutSettings panel uses animation patterns for keyboard shortcut recording:

**Recording state (pulse):**
```css
.shortcutRowRecording {
  background: color-mix(in srgb, var(--accent-primary) 8%, transparent);
  border-color: var(--glass-border-focus);
}
.shortcutKeysRecording {
  color: var(--accent-primary);
  border-color: var(--glass-border-focus);
  animation: recordingPulse 1s ease-in-out infinite;
}
@keyframes recordingPulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.4; }
}
```

**Conflict state (shake + red tint):**
```css
.shortcutRowConflict {
  background: color-mix(in srgb, var(--color-error) 12%, transparent);
  border-color: var(--color-error);
  animation: conflictShake 0.3s ease;
}
.shortcutKeysConflict {
  color: var(--color-error);
  border-color: var(--color-error);
  background: color-mix(in srgb, var(--color-error) 8%, transparent);
}
```

**Rules:**
- All colors use `color-mix(in srgb, var(--accent-primary) N%, transparent)` for tinted backgrounds — never hardcoded accent/error hex values
- Error states use `var(--color-error)` via `color-mix()` — never hardcoded red
- Key badge labels (`.shortcutKeys`) use `var(--font-mono)` + 3D asymmetric border highlights (`border-top`/`border-left`) matching the Mini-Card pattern — `var(--glass-button-bg)` background + `var(--glass-border-default)` base border
- Recording mode: set a `.recordingMode` class on the container to disable pointer cursor on non-recording rows (`cursor: default` for `.shortcutRow`, keep `cursor: pointer` for the recording row)
- Conflict auto-clears after 1.5s (JS timer), matched to the shake animation duration (0.3s)

## Before Submitting Code

Run this mental checklist for every new/changed component:

1. All `color`/`background`/`border`/`box-shadow`/`font-size` use `var(--xxx)` tokens. For `font-size`: ≥11px MUST use `--text-*` tokens; only micro-text (8/9/10px) may use raw px values. For text color: `--text-muted` is reserved for placeholders, timestamps, shortcuts, and metadata — NEVER for labels, identifiers, or descriptions the user needs to read. All WCAG AA contrast ratios: `--text-muted` ≥ 5.5:1 on `--bg-base`, `--text-secondary` ≥ 7:1. **Card elements**: inner cards inside `.liquid-glass` surfaces use `.liquid-glass-card` (height ≥50px, `--shadow-elevated` 16px阴影) or the Mini-Card pattern (height <50px, `--shadow-sm` 6px阴影 + 3D asymmetric borders). Flat `border: 1px solid var(--glass-border-default)` alone is never sufficient for glass card elements — always include `border-top: var(--glass-border-top)` and `border-left: var(--glass-border-left)` highlights.
2. No `rgba(0,0,0,x)` or `rgba(255,255,255,x)` hardcoded (except allowed exceptions)
3. `border-radius` uses the correct semantic tier: Frame→xl/2xl(24px), Panel→lg(16px), Control→md/sm(12px), Window→window(8px), Pill→full(9999px), Micro→xs(4px). Zero hardcoded pixel values (except ProgressBar's dynamic `${height/2}px`). Edge-touching elements use 0px.
4. Dialogs/popups use `var(--dialog-bg)` background + `backdrop-filter: blur()`
5. Select `<option>` elements use `var(--select-option-bg)`
6. Would look correct in all 3 themes: google-glow (dark), obsidian (darker), frosted (light)
7. Use `color-mix(in srgb, var(--color-*) N%, transparent)` for status-tinted backgrounds — never hardcode rgba
8. z-index values use `var(--z-*)` tokens — never raw numbers
9. `backdrop-filter` blur values use `var(--blur-*)` or `var(--glass-blur)` tokens
9a. `transition` values use `var(--transition-*)` tokens — never raw `0.3s ease`, `0.2s`, etc.
10. Modal/overlay backdrops use `var(--overlay-bg)` — not hardcoded black
11. All `<select>` elements use BOTH global classes: `liquid-glass-input` (bg/border/color/shadow/focus/hover/disabled) AND `liquid-glass-select` (appearance:none, arrow SVG, cursor, height, padding via `--select-height`/`--select-padding` tokens, option bg/color). All `<input>` and `<textarea>` elements use `liquid-glass-input`; `<textarea>` additionally uses `liquid-glass-textarea` (monospace font, font-size, resize, min-height, focus/placeholder). All glass button elements use `liquid-glass-button` or `GlassButton` component. CSS Modules only define layout props (width/flex) and component-specific overrides. Every input/select/button in the project should look identical regardless of which component it lives in.
12. Custom SVG data URIs (select arrows, etc.) have their hardcoded fill color noted in a comment
13. Disabled opacity: `opacity: 0.5` for buttons (`.liquid-glass-button` / `.liquid-primary-button`), `opacity: 0.4` for inputs/selects (`.liquid-glass-input`). These are managed BY the global classes — CSS Modules do not need to redefine them. (Note: `.liquid-glass-button:disabled` was added in v3.1 to close a gap where only the primary and input variants had global disabled rules.)
14. Layout surfaces (toolbar, sidebar, statusbar, terminal viewport, sendbar, transmission panel) use `liquid-glass` global class — CSS Modules for layout chrome contain ONLY layout properties. NO hand-rolled `background` / `backdrop-filter` / `border` / `box-shadow` on chrome surfaces.
15. No deprecated v2 tokens (`--glass-bg`, `--glass-border`, `--block-*`). Verify with: `grep -rn '\-\-block-' src/ --include='*.css' --include='*.tsx'`
16. Fixed-height layout bars use `align-items: center` + fixed `height` (Toolbar=36px, StatusBar=26px). Flex panels (sidebar, transmission panel, sendbar) use `height: 100%` / `flex: 1`; sendbar additionally enforces `min-height: var(--sendbar-min-height)`. Controls inside the bar use consistent vertical padding to ensure horizontal alignment.
17. Renderer components (under `src/renderers/`) must use CSS Modules + tokens — inline `React.CSSProperties` objects with hardcoded numeric values are forbidden.
18. No dead CSS Module classes — every class defined in a `.module.css` file must be referenced by the corresponding `.tsx` file.
19. Card elements use `.liquid-glass-card` when nested inside a `.liquid-glass` surface — NEVER nest `.liquid-glass` inside `.liquid-glass`. Layout chrome surfaces (sidebar, toolbar, etc.) keep using `.liquid-glass`. For elements with height <50px, use the Mini-Card pattern (module-specific CSS: `var(--shadow-sm)` + `var(--glass-fill)` + 3D asymmetric borders `border-top`/`border-left`) instead of `.liquid-glass-card`. Verify with: `grep -rn 'liquid-glass"' src/components/ --include='*.tsx'` and check that no `.liquid-glass` element is a child of another `.liquid-glass` element.

> **Dead token note**: `--glass-noise-frequency` was previously defined in tokens.css but is NOT used by the noise SVG in `global.css` (the `baseFrequency` differences between themes are negligible — all use 0.8). Do NOT define or reference this token in new themes or components.
