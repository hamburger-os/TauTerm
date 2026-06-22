---
name: tauterm-theme
description: >
  Enforce TauTerm's Liquid Glass v3 theme system when writing UI code. Use this skill whenever the user asks to create, modify, or style any React component, CSS Module, or inline style in the TauTerm project — including new dialogs, panels, buttons, inputs, sidebars, toolbars, status bars, or any visual element. Also use when the user asks about theme tokens, CSS variables, dark/light mode, or wants to ensure a component looks correct across themes. This skill ensures zero hardcoded colors and full theme compatibility across google-glow, obsidian, and frosted themes. Covers global utility classes (liquid-glass, liquid-primary-button, liquid-glass-button, liquid-glass-input), v3 asymmetric border highlights, SVG noise texture with per-theme opacity, accent-holofoil-gradient for primary actions, and color-mix() pattern for status-tinted backgrounds.
license: MIT
metadata:
  author: tauterm
  version: "1.4"
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
font-size: var(--text-sm);         /* 12px */
font-size: var(--text-base);       /* 13px */
font-size: var(--text-md);         /* 14px */
font-size: var(--text-lg);         /* 16px */
font-size: var(--text-xl);         /* 18px */
border-radius: var(--radius-xs);   /*  3px — scrollbars only */
border-radius: var(--radius-sm);   /*  6px */
border-radius: var(--radius-md);   /* 10px */
border-radius: var(--radius-lg);   /* 14px */
border-radius: var(--radius-xl);   /* 18px */
border-radius: var(--radius-2xl);  /* 24px */
border-radius: var(--radius-full); /* 9999px — toggles, pills */
padding: var(--spacing-md);        /* 12px (xs:4 sm:8 md:12 lg:16 xl:24 2xl:32) */
transition: all var(--transition-fast);  /* 150ms ease */
transition: all var(--transition-normal); /* 300ms ease */
transition: all var(--transition-button); /* 0.3s cubic-bezier(0.4, 0, 0.2, 1) — button hover lift */
transition: all var(--transition-input);  /* 0.3s ease — input focus glow */
z-index: var(--z-sidebar);         /* 10 */
z-index: var(--z-panel);           /* 20 */
z-index: var(--z-overlay);         /* 30 */
z-index: var(--z-toast);           /* 50 */
blur: var(--blur-xs);              /*  4px — overlay backdrops (shared constant) */
```

### Theme Tokens (Level 2 — vary per theme)

**Text:**
```css
color: var(--text-primary);     /* main text */
color: var(--text-secondary);   /* secondary text */
color: var(--text-muted);       /* dim text — use sparingly, NOT for small controls */
```

**Accent:**
```css
color: var(--accent-primary);
color: var(--accent-secondary);
background: var(--accent-gradient);  /* blue→indigo */
box-shadow: 0 0 10px var(--accent-glow);
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
background: var(--glass-fill);         /* v3 primary — glass fill gradient */
background: var(--glass-bg);           /* v2 alias — still works */
backdrop-filter: blur(var(--glass-blur)) saturate(var(--glass-blur-saturate));
border: 1px solid var(--glass-border-default);  /* v3 primary */
border: 1px solid var(--glass-border);          /* v2 alias */
border-top: 1px solid var(--glass-border-top);
border-left: 1px solid var(--glass-border-left);
box-shadow: var(--glass-shadow-outer), var(--glass-shadow-inner);
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

**Layout blocks:**
```css
background: var(--block-toolbar-bg);     /* Toolbar area */
background: var(--block-sidebar-bg);     /* Sidebar area */
background: var(--block-terminal-bg);    /* Terminal viewport */
background: var(--block-sendbar-bg);     /* SendBar area */
background: var(--block-statusbar-bg);   /* StatusBar area */
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
| `.liquid-glass` | Full glass panel: SVG noise texture (via ::before with per-theme opacity) + asymmetric top/left highlight borders + multi-layer shadows + saturate filter. Used by `GlassPanel` component automatically. |
| `.liquid-primary-button` | Primary action button: holographic gradient with `gradient-shift` animation, glass blur, white text. Use for Send, Connect, Submit buttons. |
| `.liquid-glass-button` | Secondary glass button: semi-transparent bg, hover lift effect. Use for Cancel, Close, auxiliary actions. |
| `.liquid-glass-input` | Glass input: dark inset bg, accent glow on focus. Use for search bars, text inputs. |
| `.glow-orb` | Animated background orb. Used internally by `GoogleGlowBackground`. |

```tsx
// ✅ Combine CSS Module + global utility class
<button className={`${styles.sendBtn} liquid-primary-button`}>Send</button>
<input className={`${styles.search} liquid-glass-input`} />
<div className={`${styles.card} liquid-glass`}>Content</div>
```

## Component Patterns

### CSS Module Component

```css
/* ✅ Correct — v3 primary tokens */
.myComponent {
  background: var(--glass-fill);
  border: 1px solid var(--glass-border-default);
  border-radius: var(--radius-md);
  color: var(--text-primary);
  padding: var(--spacing-md);
}
.myComponent:hover {
  background: var(--glass-bg-hover);
  border-color: var(--glass-border-hover);
}

/* ❌ Wrong */
.myComponent {
  background: rgba(0, 0, 0, 0.2);   /* hardcoded, won't work in frosted theme */
  border: 1px solid #fff;            /* invisible on light background */
  color: #888;                       /* fixed, won't change with theme */
  border-radius: 4px;                /* too small for glass feel */
}
```

### Inline Style Component

```tsx
// ✅ Correct
<div style={{
  backgroundColor: "var(--bg-base)",
  color: "var(--text-secondary)",
  border: "1px solid var(--glass-border-default)",
  borderRadius: "var(--radius-md)",
}} />

// ❌ Wrong
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
  border-radius: var(--radius-xl);
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

### New Select Dropdown Template

**Preferred: combine CSS Module + global `.liquid-glass-input` class**

```css
/* CSS Module — select-specific + sizing only */
.select {
  appearance: none;
  -webkit-appearance: none;
  width: 100%;
  padding: 6px 28px 6px 10px;
  border-radius: var(--radius-md);
  font-size: var(--text-sm);
  font-family: var(--font-ui);
  cursor: pointer;
  background-image: var(--select-arrow);
  background-repeat: no-repeat;
  background-position: right 10px center;
}
.select option {
  background: var(--select-option-bg);
  color: var(--text-primary);
}
```

```tsx
// JSX — `.liquid-glass-input` provides bg/border/color/shadow/focus/hover/disabled
<select className={`${styles.select} liquid-glass-input`}>
  <option value="a">A</option>
</select>
```

> `.liquid-glass-input` already handles: `background`, `border`, `box-shadow`, `color`,
> `outline`, `:focus` glow, `:hover` border, `:disabled` transparency.
> CSS Modules only need to define select-specific props and component-level sizing.

### Status-Tinted Background (color-mix pattern)

Use `color-mix()` to create theme-adaptive tinted backgrounds. This is the canonical pattern for error boxes, warning banners, success indicators, and signal badges:

```css
/* ✅ Correct — adapts to all 3 themes */
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

/* ❌ Wrong — would need different rgba per theme */
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
  transition: all 0.3s cubic-bezier(0.4, 0, 0.2, 1);
}
/* Slider knob */
.toggleTrack::after {
  content: "";
  position: absolute;
  top: 2px; left: 2px;
  width: 11px; height: 11px;
  border-radius: 50%;
  background: var(--text-muted);
  transition: all 0.3s cubic-bezier(0.4, 0, 0.2, 1);
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

```css
.statusDot {
  width: 7px; height: 7px;
  border-radius: 50%;
  background: var(--text-muted);
}
.statusDot.connected {
  background: var(--color-success);
  box-shadow: 0 0 10px var(--color-success);
  animation: pulse 2s ease-in-out infinite;
}
```

## Before Submitting Code

Run this mental checklist for every new/changed component:

1. All `color`/`background`/`border`/`box-shadow` use `var(--xxx)` tokens — no raw hex, no raw rgba
2. No `rgba(0,0,0,x)` or `rgba(255,255,255,x)` hardcoded (except allowed exceptions)
3. `border-radius` uses `var(--radius-md)` or larger (minimum 6px, prefer 10px+)
4. Dialogs/popups use `var(--dialog-bg)` background + `backdrop-filter: blur()`
5. Select `<option>` elements use `var(--select-option-bg)`
6. Would look correct in all 3 themes: google-glow (dark), obsidian (darker), frosted (light)
7. Use `color-mix(in srgb, var(--color-*) N%, transparent)` for status-tinted backgrounds — never hardcode rgba
8. z-index values use `var(--z-*)` tokens — never raw numbers
9. `backdrop-filter` blur values use `var(--blur-*)` or `var(--glass-blur)` tokens
10. Modal/overlay backdrops use `var(--overlay-bg)` — not hardcoded black
11. All `<select>` and `<input>` elements use the global `liquid-glass-input` class for base visuals (bg/border/color/shadow/focus/hover/disabled). CSS Modules only define select-specific props (appearance, arrow, option bg) and component-level sizing. Every select/input in the project should look identical regardless of which component it lives in.
12. Custom SVG data URIs (select arrows, etc.) have their hardcoded fill color noted in a comment
13. Disabled opacity: use `opacity: 0.5` for buttons, `opacity: 0.4` for inputs/selects — be consistent across components

> **Dead token note**: `--glass-noise-frequency` was previously defined in tokens.css but is NOT used by the noise SVG in `global.css` (the `baseFrequency` differences between themes are negligible — all use 0.8). Do NOT define or reference this token in new themes or components.
