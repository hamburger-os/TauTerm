# TauTerm Theme Skill — Source References

Key source files referenced by the tauterm-theme skill:

- [src/styles/tokens.css](../../../src/styles/tokens.css) — All CSS custom properties (Level 1 shared constants + Level 2 per-theme tokens)
- [src/styles/global.css](../../../src/styles/global.css) — Global utility classes (`.liquid-glass`, `.liquid-glass-button`, `.liquid-glass-input`, `.liquid-primary-button`, `.glow-orb`) and keyframes
- [src/context/ThemeContext.tsx](../../../src/context/ThemeContext.tsx) — Theme provider with legacy migration (neon-dark/ocean/sunset → google-glow)
- [src/components/Layout/GoogleGlowBackground.tsx](../../../src/components/Layout/GoogleGlowBackground.tsx) — Dynamic orb background component
- [docs/theme-guide.md](../../../docs/theme-guide.md) — Comprehensive theme development guide with token reference tables
