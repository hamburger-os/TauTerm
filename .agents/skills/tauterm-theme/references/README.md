# TauTerm Theme Skill — Source References

This directory is non-normative. The **only rules source** is `../SKILL.md`.

Implementation references:

- [src/styles/tokens.css](../../../src/styles/tokens.css) — Theme + visual-performance tokens
- [src/styles/global.css](../../../src/styles/global.css) — Material classes, lightweight ambient glow, compatibility/motion fallbacks
- [src/context/ThemeContext.tsx](../../../src/context/ThemeContext.tsx) — Theme, performance tier, and motion state
- [src/components/Layout/GoogleGlowBackground.tsx](../../../src/components/Layout/GoogleGlowBackground.tsx) — Transform-only radial-gradient ambient glow
- [src/components/Terminal/TerminalView.tsx](../../../src/components/Terminal/TerminalView.tsx) — Terminal visibility/paint policy
- [src/components/Layout/SplitView.tsx](../../../src/components/Layout/SplitView.tsx) — Per-pane content surface assignment
- [src/assets/icons/prompts.md](../../../src/assets/icons/prompts.md) — Icon generation prompts
