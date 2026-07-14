---
name: tauterm-theme-review
description: >
  Audit and review TauTerm UI components for theme consistency across all 3 themes (google-glow, obsidian, frosted). Use this skill whenever the user asks to review, audit, check, or verify theme compatibility — including "review styles", "check theme consistency", "audit cross-theme", "verify frosted/light theme", "检查主题" (check theme), "审查样式" (review styles), "主题兼容性" (theme compatibility), or any request to ensure components look correct across themes. This is a QA/audit skill that examines EXISTING code and produces structured fix reports — distinct from tauterm-theme which enforces rules when writing NEW code. The light (frosted) theme is the most common source of hidden bugs since developers primarily work in dark mode.
license: MIT
metadata:
  author: tauterm
  version: "1.1"
---

# TauTerm Theme Style Review

> **语言偏好**：所有审查报告默认使用中文（简体中文）输出。

A systematic audit skill that reviews existing UI components for theme consistency across all 3 TauTerm themes: google-glow (dark), obsidian (darker), frosted (light).

## 用途

与 `tauterm-theme`（在编写新代码时强制执行规则）不同，此 skill **审查**已有组件，发现开发过程中不可见的主题 bug —— 尤其是在日常开发中很少使用的 frosted（浅色）主题下的问题。

## 审查范围

目标目录：`src/components/`、`src/renderers/`、`src/App.css`、`src/styles/`

忽略：`node_modules/`、`dist/`、`src-tauri/`、测试文件

## Audit Workflow

### Step 1: Determine Scope

Ask the user what to audit:
- **Single component**: e.g., "review SendBar theme compatibility"
- **Component group**: e.g., "audit all FileTransfer components"
- **Layout chrome**: toolbar, sidebar, statusbar, sendbar, terminal viewport
- **Full project**: every component under `src/components/` and `src/renderers/`

### Step 2: Gather Files — 高效批量审计策略

对于**全量项目**审计，先跑批量 grep 快速定位问题文件，再深入阅读：

```bash
# 第一阶段：批量扫描（并行运行，30 秒内完成全项目扫描）
# 硬编码色（排除已知例外后）
grep -rn '#[0-9a-fA-F]\{6\}' src/components/ src/renderers/ --include='*.css'
grep -rn 'rgba\?(' src/components/ src/renderers/ --include='*.css'

# 废弃令牌
grep -rn '\-\-glass-border[^-]' src/components/ src/renderers/ --include='*.css' --include='*.tsx'
grep -rn '\-\-glass-bg[^-]' src/components/ src/renderers/ --include='*.css' --include='*.tsx'
grep -rn '\-\-block-' src/components/ src/renderers/ --include='*.css' --include='*.tsx'

# 裸数字
grep -rn 'font-size:\s*\(1[1-9]\|[2-9][0-9]\)px' src/components/ src/renderers/ --include='*.css'
grep -rn 'z-index:\s*\d\+' src/components/ src/renderers/ --include='*.css'
grep -rn 'border-radius:\s*\d\+px' src/components/ src/renderers/ --include='*.css'

# 浮层 blur 令牌一致性（用于检查 A8）
grep -rn 'blur(var(--blur-heavy))' src/components/ src/renderers/ --include='*.css'
grep -rn 'blur(var(--glass-blur))' src/components/ src/renderers/ --include='*.css'

# --text-muted 在功能标签上的误用
grep -rn 'color:\s*var(--text-muted)' src/components/ --include='*.css'

# 卡片一致性 — 内层卡片缺失 .liquid-glass-card（F8：交叉比对 var(--glass-fill) 与 TSX className）
grep -rn 'var(--glass-fill)' src/components/ src/renderers/ --include='*.css'

# 卡片一致性 — 元素缺失 3D 不对称边框高光（F10：仅有平面 border，无 border-top/border-left）
grep -rn 'border: 1px solid var(--glass-border-default)' src/components/ src/renderers/ --include='*.css'

# 卡片一致性 — Mini-Card 阴影层级错误（F9：<50px 元素使用了 --shadow-elevated）
grep -rn 'box-shadow:\s*var(--shadow-elevated)' src/components/ src/renderers/ --include='*.css'
# ↑ 交叉比对每个匹配结果：如果元素高度 <50px → 应降级为 var(--shadow-sm)

# 卡片一致性 — Card 阴影层级确认（F9 反向：--shadow-sm 用于 ≥50px 元素？）
grep -rn 'box-shadow:\s*var(--shadow-sm)' src/components/ src/renderers/ --include='*.css'
# ↑ 交叉比对：如果元素高度 ≥50px → 应升级为 var(--shadow-elevated) 或使用 .liquid-glass-card
```

对于**单个组件**审计：
1. 找出组件的 `.module.css` 和对应 `.tsx` 文件
2. 读取 `src/styles/tokens.css` — 所有 CSS 自定义属性的事实来源
3. 读取 `src/styles/global.css` — 了解全局类提供哪些属性
4. 对比参考 `docs/theme-guide.md` 和 `.claude/skills/tauterm-theme/SKILL.md`

### Step 3: Run Checks

Execute the checklist below against each file pair. Apply the known exceptions (Section: Known Exceptions) to avoid false positives. A check PASSES when the component follows the rule for all 3 themes.

### Step 4: Produce Report

Output a structured report using the format defined in "Report Format" below.

### Step 5: Recommend Fixes

For each finding, suggest the specific fix referencing the pattern from `tauterm-theme` skill. Prioritize CRITICAL → HIGH → MEDIUM → LOW.

---

## Audit Checklist

### Category A: Hardcoded Values (CRITICAL)

These cause the component to look wrong in at least one theme — usually frosted (light).

| ID | Check | Detection | Exception |
|----|-------|-----------|-----------|
| **A1** | Hardcoded color values | grep `#[0-9a-fA-F]{3,8}\b` and `rgba?\(` in CSS/TSX | Google orb colors (#4285F4, #EA4335, #FBBC05, #34A853), `color: #fff` on holofoil/accent buttons, SVG data URIs with documented fill colors |
| **A2** | Dark-only background assumptions | Look for `rgba(0, 0, 0, ...)` that would be invisible on frosted's `#f8fafc` bg | Explicit overlay backdrops using `var(--overlay-bg)` |
| **A3** | font-size raw px ≥ 11px | grep `font-size:\s*1[1-9]px` and `font-size:\s*[2-9][0-9]px` | Micro-text at 8/9/10px is allowed — see Micro-Text Exceptions table below |
| **A4** | Transition raw values | grep `transition:.*\d+ms\s+ease` and `transition:.*\d+\.\d+s` not using `var(--transition-*)` | Transitions on motion.div (framer-motion uses its own API) |
| **A5** | z-index raw numbers | grep `z-index:\s*\d+` — must use `var(--z-sidebar)`, `var(--z-panel)`, `var(--z-overlay)`, `var(--z-toast)` | None — all z-index values must use tokens |
| **A6** | backdrop-filter blur raw values | grep `blur\(\d+px\)` — must use `var(--blur-*)` or `var(--glass-blur)` | GoogleGlowBackground orbs use `var(--bg-orb-blur)` |
| **A7** | border-radius raw px | grep `border-radius:\s*\d+px` | Dynamic values (e.g., ProgressBar `height/2`), `0px` for edge-contact elements, `border-radius: 50%` for circles. Window chrome corners use `--radius-window` (8px) |
| **A8** | Floating element blur 令牌不一致 | grep `blur(var(--blur-heavy))` in CSS — 浮动元素（position:absolute/fixed）应使用 v3 主令牌 `--glass-blur` 而非 `--blur-heavy` | `--blur-heavy` 在 google-glow=24px 而 `--glass-blur`=25px，差异极小但统一使用主令牌保持一致性 |

### Category B: Missing Global CSS Utility Classes (HIGH)

These classes provide the unified glass-morphism effect. Missing them causes visual inconsistency.

| ID | Check | Detection | Fix |
|----|-------|-----------|-----|
| **C1** | Layout chrome missing `.liquid-glass` | Check that toolbar, sidebar, right sidebar, statusbar, terminal viewport, sendbar, transmission panel have `liquid-glass` in className | Add `liquid-glass` to the element's className; keep CSS Module with layout properties only |
| **C2** | Input/select missing global glass class | Check all `<input>`, `<select>`, `<textarea>` elements | `<input>`/`<textarea>`: add `liquid-glass-input`. `<select>`: add BOTH `liquid-glass-input` + `liquid-glass-select`. CSS Module should only define layout/sizing props |
| **C3** | Glass button missing `.liquid-glass-button` | Check `<button>` elements that render with glass visual style | Add `liquid-glass-button` class or use `GlassButton variant="secondary"` |
| **C4** | Primary action missing `.liquid-primary-button` | Check primary CTA buttons (Connect, Send, Submit) | Add `liquid-primary-button` class or use `GlassButton variant="primary"` |

### Category D: Incorrect Token Selection (MEDIUM)

These degrade design consistency but the component still works in all themes.

| ID | Check | Detection | Fix |
|----|-------|-----------|-----|
| **D1** | Wrong border-radius tier | Verify each element uses the correct semantic tier token | Frame→xl/2xl(24px), Panel→lg(16px), Control→md/sm(12px), Window→window(8px), Pill→full(9999px), Micro→xs(4px) |
| **D2** | Text color semantic misuse | Check if `--text-muted` is used for functional labels users need to read. 重点检查 `.groupLabel`、`.sectionLabel`、`.fieldLabel` 等表单标签类名 | Use `--text-secondary` for functional labels; `--text-muted` only for placeholders/timestamps/shortcuts/metadata. 经验法则：表单标签、分组标题、字段名称 → `--text-secondary`；占位符、快捷键、版本号 → `--text-muted` |
| **D3** | Missing `color-mix()` for status tints | Check if status-colored backgrounds use hardcoded rgba | Use `color-mix(in srgb, var(--color-*) N%, transparent)` pattern |
| **D4** | Dialog missing backdrop-filter | Check dialogs/popups for `backdrop-filter: blur(...)` and `-webkit-backdrop-filter: blur(...)` | Add both properties using `var(--glass-blur)` |

### Category E: Layout Bar Alignment (MEDIUM)

These cause visual misalignment between controls in layout chrome bars.

| ID | Check | Detection | Fix |
|----|-------|-----------|-----|
| **E1** | `align-items` not `center` | Check layout bar CSS Modules for `align-items: flex-end` or `flex-start` | Use `align-items: center` |
| **E2** | `min-height` instead of fixed `height` | Check layout bar CSS Modules | Use fixed `height` (Toolbar=40px, StatusBar=26px). Panels (sidebar, transmission panel, sendbar) use `height: 100%` / `flex: 1`. Sendbar additionally enforces `min-height: var(--sendbar-min-height)` — this is intentional and should NOT be flagged. |

> **Note**: The `tauterm-theme` skill says Toolbar=36px, but the actual code uses 40px. Either value is acceptable if consistent — flag the discrepancy for the user to decide. Sendbar was migrated from fixed 40px to a flex-based resizable panel (min-height: var(--sendbar-min-height), 106px) as part of the SendBar modular refactoring — E2 should exempt sendbar the same way it exempts sidebar and transmission panel.

### Category F: Component-Specific Issues (LOW)

These are code quality concerns that don't directly break visual appearance.

| ID | Check | Detection | Fix |
|----|-------|-----------|-----|
| **F1** | Missing disabled state | Check interactive elements for `:disabled` / `disabled` handling | Ensure disabled state reduces opacity/contrast |
| **F2** | Inline hardcoded styles in renderers | Check `src/renderers/` files for `style={{...}}` with hardcoded numeric values | Convert to CSS Module + tokens |
| **F3** | Dead CSS Module classes | For each `.module.css`, verify every class is referenced in the `.tsx` | Remove unused classes |
| **F4** | `.liquid-glass` on absolute/fixed elements | Check for className containing `liquid-glass` on elements with `position: absolute` or `position: fixed` in CSS Module | Remove `.liquid-glass` and inline the glass properties in the CSS Module using v3 tokens |
| **F5** | Select missing option background | Check `<select>` elements — if they use `liquid-glass-select` class, option bg/color is auto-handled | If NOT using `liquid-glass-select`: add `.mySelect option { background: var(--select-option-bg); color: var(--text-primary); }`. If using `liquid-glass-select`: verify the select element has BOTH `liquid-glass-input liquid-glass-select` classes |
| **F6** | Undocumented SVG data URI colors | Check for `url("data:image/svg+xml,...")` without a comment noting the hardcoded fill | Add comment: `/* fill color: #xxxxxx — matches --text-muted */` |
| **F7** | `.liquid-glass` nested inside `.liquid-glass` | Manual review of component JSX tree — check if any element with `liquid-glass` class is a descendant of another `liquid-glass` element | Replace inner `.liquid-glass` with `.liquid-glass-card`. Only layout chrome surfaces (sidebar, toolbar, terminal, statusbar, sendbar, transmission panel) should use `.liquid-glass` at the outermost level |
| **F8** | Card missing `.liquid-glass-card` or wrong shadow tier | Check inner elements nested in `.liquid-glass` surfaces that have `var(--glass-fill)` background. If height ≥50px: must use `.liquid-glass-card` (provides `--shadow-elevated`). If height <50px: must use Mini-Card pattern with `var(--shadow-sm)` — using `--shadow-elevated` on small elements is wrong. grep for `var(--glass-fill)` in CSS Modules, cross-reference with TSX className | For ≥50px elements: add `.liquid-glass-card`. For <50px elements: use Mini-Card pattern (module-specific CSS with `var(--shadow-sm)` + 3D asymmetric borders) |
| **F9** | Mini-card missing 3D borders or wrong shadow tier | Check small elements with `var(--shadow-sm)` — if they have only flat `border: 1px solid var(--glass-border-default)` without `border-top`/`border-left` highlights. Also check elements <50px using `var(--shadow-elevated)` (wrong tier for size — should be `var(--shadow-sm)`). grep for `var(--shadow-elevated)` in CSS Modules and manually verify element height | Add `border-top: 1px solid var(--glass-border-top)` and `border-left: 1px solid var(--glass-border-left)`. If `--shadow-elevated` is on a <50px element, downgrade to `var(--shadow-sm)` |
| **F10** | Card element using `var(--glass-fill)` with flat borders | Check any element using `var(--glass-fill)` background — must also have 3D asymmetric borders (`border-top` + `border-left` highlights) or be using `.liquid-glass-card` (which provides them). Flat `border: 1px solid var(--glass-border-default)` alone on a `glass-fill` background is the most common card consistency bug. grep for `var(--glass-fill)` in CSS Modules, verify each has `border-top` and `border-left` or uses `.liquid-glass-card` in TSX | Add `border-top: 1px solid var(--glass-border-top)` and `border-left: 1px solid var(--glass-border-left)`. If element is ≥50px and doesn't use `.liquid-glass-card`, add the global class instead |

### Category G: Frosted-Specific Issues (CRITICAL)

These are bugs that ONLY appear in the frosted (light) theme. They are the highest priority because they're invisible during normal development.

| ID | Check | Detection | Fix |
|----|-------|-----------|-----|
| **G1** | Invisible borders on light bg | Inspect border tokens — `rgba(255,255,255,x)` borders become invisible on frosted's `#f8fafc` background | Always use theme tokens for borders; frosted theme defines appropriate `rgba(148,163,184,x)` borders |
| **G2** | Poor text contrast in frosted | Compare `--text-muted` contrast ratios across themes; frosted light text may be too faint on light bg | Verify WCAG AA ≥ 4.5:1 for text-muted on `--bg-base` in frosted theme |
| **G3** | Dark-only `rgba(0,0,0,x)` backgrounds | grep `rgba\(\s*0,\s*0,\s*0` in CSS — these create dark patches invisible on dark themes but very visible on light | Use `var(--glass-*-bg)` tokens or `color-mix()` pattern |
| **G4** | `mix-blend-mode: screen` on light bg | Check if `screen` blend mode is used outside `GoogleGlowBackground` orbs | Each theme has its own `--bg-orb-blend` (screen for dark, multiply for frosted); don't hardcode |
| **G5** | Hardcoded overlay opacity | Check overlay/modal backdrops for hardcoded `rgba(0,0,0,x)` opacity | Use `var(--overlay-bg)` — frosted has lower opacity (0.2 vs 0.5/0.6 in dark themes) |
| **G6** | Undetectable glass fill in frosted | Elements with `var(--glass-fill)` but no `box-shadow` — the frosted theme's shadows are extremely subtle (4-6px); without shadow, the glass fill blends into the parent surface and looks flat | Ensure all glass elements have at least `var(--shadow-sm)`; for larger cards use `.liquid-glass-card` (which provides `var(--shadow-elevated)`) |

### Category H: DualPane Component Issues (MEDIUM)

DualPane (`src/components/Terminal/DualPane.tsx` + `DualPane.module.css`) has specific theme constraints:

| ID | Check | Detection | Fix |
|----|-------|-----------|-----|
| **H1** | Hardcoded `font-family` in `.container` | grep `font-family:` in `DualPane.module.css` — must be `var(--font-mono)`, not raw font stacks | Replace with `font-family: var(--font-mono);` |
| **H2** | CSS custom properties (e.g., `--dual-*`) | grep `--dual-` in `DualPane.module.css` and `DualPane.tsx` — must NOT exist | Remove CSS custom properties; set layout values via React inline style (`fontSize`, `width`, `left`) on individual elements |
| **H3** | TX row not using `--accent-secondary` | Check `.txRow` selector — must use `color: var(--accent-secondary);` | Ensure `.txRow { color: var(--accent-secondary); }` |
| **H4** | Divider not using `--glass-border-*` tokens | Check `.divider` and `.dividerActive`/`:hover`/`:active` — must use `--glass-border-default` (default) and `--glass-border-hover` (active) | Replace hardcoded border colors with glass tokens |
| **H5** | Timestamp tag using wrong token | Check `.tsTag` — must use `color: var(--text-secondary); opacity: 0.6;` (not `--text-muted`) | Fix to `var(--text-secondary)` + `opacity: 0.6` |
| **H6** | Missing ARIA on divider | Check divider element in `DualPane.tsx` — must have `role="separator"`, `aria-orientation="vertical"`, `aria-valuenow`, `aria-valuemin/max`, `tabIndex={0}` | Add ARIA attributes |
| **H7** | Scrollbar using hardcoded color | Check `.scrollArea` webkit scrollbar selectors — must use `var(--glass-border-default)` | Replace with glass token |

---

## Report Format（审查报告格式）

审查报告必须使用中文，按以下结构输出：

```markdown
# TauTerm 主题样式审查报告

**审查范围**：<审查的组件或目录>
**日期**：<今天日期>
**检查主题**：google-glow（炫彩流光）、obsidian（黑曜石）、frosted（白霜）

## 摘要

| 严重程度 | 数量 |
|----------|------|
| [严重] CRITICAL | N |
| [高] HIGH     | N |
| [中] MEDIUM   | N |
| [低] LOW      | N |
| **合计**    | **N** |

## 按组件分类的发现

### <组件名>（`<文件路径>`）

| ID | 检查项 | 严重程度 | 位置 | 问题描述 | 修复建议 |
|----|--------|----------|------|----------|----------|
| A1 | 硬编码颜色 | CRITICAL | `file.module.css:42` | `color: #888` 不随主题变化 | 替换为 `color: var(--text-secondary)` |

## 表现优异组件

以下组件在全部检查中均表现完美，可作为项目主题实施的参考范例：

> <填入通过全部检查的组件列表>

## 批量修复命令

```bash
# 验证无残留 v2 别名令牌
grep -rn '\-\-block-' src/ --include='*.css' --include='*.tsx'
```
```

## Known Exceptions

These patterns trigger checks but are INTENTIONALLY allowed. Refer to this table before flagging any finding.

### Allowed Hardcoded Colors

| Value | Where | Why |
|-------|-------|-----|
| `#4285F4, #EA4335, #FBBC05, #34A853` | `GoogleGlowBackground.tsx` | Google brand colors — fixed design feature |
| `#fff` | `.liquid-primary-button`, `.primary` variant, `--text-on-accent` references | Always white text on holographic/accent backgrounds |
| SVG data URI fill colors | `tokens.css` `--select-arrow` definitions | Documented per-theme in tokens.css comments |

### Layout Chrome Surfaces (MUST use `.liquid-glass`)

These 7 surfaces use the `className={styles.xxx + ' liquid-glass'}` pattern — CSS Modules contain ONLY layout properties:

- `Toolbar.tsx` (Toolbar.module.css)
- `SessionSidebar.tsx` (SessionSidebar.module.css)
- `RightSidebar.tsx` (RightSidebar.module.css)
- `StatusBar.tsx` (StatusBar.module.css)
- `TerminalView.tsx` (Terminal.module.css — the viewport container)
- `SendBar.tsx` (SendBar.module.css)
- `AutoReplyPanel.tsx` (AutoReplyPanel.module.css) — nested inside SendBar liquid-glass surface
- `ScriptEditor.tsx` (ScriptEditor.module.css) — nested inside SendBar liquid-glass surface
- `TransmissionPanel.tsx` (TransmissionPanel.module.css)

### Inner Card Surfaces (use `.liquid-glass-card` or Mini-Card pattern)

These elements are nested inside a `.liquid-glass` layout surface and must NOT use `.liquid-glass`:

- `AggregateProgress.tsx` — nested inside TransmissionPanel, uses `.liquid-glass-card`
- `ConnectDialog.tsx` `.modeCard` — nested inside ConnectDialog (`.liquid-glass`), now uses `.liquid-glass-card` for consistent `shadow-elevated` + 3D borders
- `StatsDashboardRenderer` `.card` — nested inside terminal viewport (`.liquid-glass`), now uses `.liquid-glass-card` for glass consistency (was previously `var(--bg-secondary)` solid)
- `PerFileList.tsx` (`.row` elements) — uses Mini-Card pattern (`var(--shadow-sm)` 6px shadow + 3D asymmetric borders), consistent with other small elements in the TransmissionPanel (`.fileSummary`, `.errorBox`, etc.)

### Tool Panel Components (use `--color-accent`, `--glass-hover`, `--glass-fill-secondary`, `--text-tertiary`)

These components are nested inside `RightSidebar.tsx` (`.liquid-glass`) via `RightSidebarPanel` accordion wrappers. They use per-theme tool panel helper tokens defined in `tokens.css`:

- `CalculatorTool.tsx` → `CalculatorTool.module.css` — 3-tab container (checksum/encoding/bitops)
- `ChecksumTool.tsx` → `ChecksumTool.module.css` — CRC/checksum calculator with mode/algorithm buttons
- `EncodingTool.tsx` → `EncodingTool.module.css` — 16 encoding conversion operations
- `BitOpsTool.tsx` → `BitOpsTool.module.css` — bitwise operations + C sizeof parser
- `ProtocolTool.tsx` → `ProtocolTool.module.css` — Modbus RTU/ASCII, AT response parser

**Token usage in these components:**
- `--color-accent` — active tab/mode button fill (with `--text-on-accent` text)
- `--glass-hover` — panel header hover background
- `--glass-fill-secondary` — result card backgrounds, button default state
- `--text-tertiary` — dim hint/placeholder text
- Inputs/selects use `liquid-glass-input` global class
- Monospace values use `var(--font-mono)` with `var(--text-xs)`

### Floating Elements (CANNOT use `.liquid-glass`)

These elements have `position: absolute` or `position: fixed` and must inline glass properties:

| Component | CSS File | Position |
|-----------|----------|----------|
| `Toast` | `Toast.module.css` | `fixed` |
| `ContextMenu` | `ContextMenu.module.css` | `fixed` |
| `SearchBar` | `SearchBar.module.css` | `absolute` |
| `SendBar` history dropdown | `SendBar.module.css` | `absolute` |
| `AutoReplyRuleEditor` modal | `AutoReplyRuleEditor.module.css` | `fixed` (createPortal) |
| `AutoReplyPanel` rename/import modals | `AutoReplyPanel.module.css` | `fixed` (createPortal) |
| `ScriptEditor` rename/import modals | `ScriptEditor.module.css` | `fixed` (createPortal) |
| `MacroPicker` dropdown | `AutoReplyRuleEditor.module.css` | `absolute` |
| `MatchTester` regex panel | `AutoReplyRuleEditor.module.css` | `absolute` (inside modal) |
| `ConnectDialog` mode badges | `ConnectDialog.module.css` | `absolute` |
| `ScrollToBottomButton` | `ScrollToBottomButton.module.css` | `absolute` |

### Transparent Buttons (no `.liquid-glass-button` required)

These buttons intentionally use transparent/ghost styling and don't need the glass button class:

- TitleBar window control buttons (`.windowBtn`)
- SessionSidebar list items (the session tab buttons)
- Settings sidebar nav items
- ConnectDialog mode cards
- CommandPalette result items
- GlassButton `variant="ghost"`
- SendBar history items
- ZmodemConfigForm toggle buttons

### Micro-Text Exceptions (8/9/10px allowed)

font-size 8/9/10px is the "micro" tier — intentionally small for status bars, badges, and compact labels:

| Size | Locations |
|------|-----------|
| 8px | `StatusBar.module.css` (`.signalDot`, `.modeBadge`), `ConnectDialog.module.css` (mode badge) |
| 9px | `StatusBar.module.css` (`.paramText`, `.uptimeText`, `.statItem`), `TransmissionPanel.module.css` (`.sectionLabel`), `CommandPalette.module.css` (`.groupTitle`), `SendBar.module.css` (`.historyMode`) |
| 10px | `StatusBar.module.css` (`.bar` base), `TransmissionPanel.module.css` (`.fileSummary`, `.errorBox`, `.placeholder`), `SettingsPage.module.css` (`.settingDesc`), `CommandPalette.module.css` (`.groupTitle`), `TitleBar.module.css` (`.title`), `SessionSidebar.module.css` (session meta), `SendBar.module.css` (`.historyItem`) |

### Border-Radius Exceptions

| Case | Value | Why |
|------|-------|-----|
| ProgressBar fill | `${height/2}px` | Dynamic — must be computed at runtime |
| Edge-contact elements | `0px` | Element outer edge touches screen/parent boundary |
| Circle indicators | `50%` | Circular dots (status indicators) |
| Zero-radius corners | `0 0 0 var(--radius-md)` | SearchBar partial rounding (one corner matches terminal) |

### Decorative Icon Font-Size Exceptions

装饰性 emoji/icon 字号可超出 `--text-xl`(20px) 范围，因为它们不是正文文本：

| Value | Where | Why |
|-------|-------|-----|
| `28px` | `ConnectDialog.module.css` `.modeIcon` | 模式选择卡片装饰 emoji |
| `32px` | `Terminal.module.css` `.emptyIcon` | 空状态装饰 emoji |

> **要求**：此类例外需在 CSS 中添加注释 `/* 装饰性 emoji 图标，不受字号令牌约束 */`

---

## Pre-Audit Reference

Before running any audit, read these files for ground truth:

1. **`src/styles/tokens.css`** — All ~60 CSS custom properties per theme
2. **`src/styles/global.css`** — What each global class provides (`.liquid-glass`, `.liquid-glass-button`, `.liquid-glass-input`, `.liquid-primary-button`, `.glass-overlay`)
3. **`docs/theme-guide.md`** — Full token reference with v2→v3 migration table, WCAG contrast tables, component patterns
4. **`.claude/skills/tauterm-theme/SKILL.md`** — The companion enforcement skill (18-rule checklist for new code)

### Quick Token Lookup

```bash
# View all tokens for a specific theme
grep -A 100 '\[data-theme="google-glow"\]' src/styles/tokens.css
grep -A 100 '\[data-theme="frosted"\]' src/styles/tokens.css

# Find deprecated token usage (should return zero results)
grep -rn '\-\-glass-border[^-]' src/components/ src/renderers/ --include='*.css' --include='*.tsx'
grep -rn '\-\-glass-bg[^-]' src/components/ src/renderers/ --include='*.css' --include='*.tsx'
grep -rn '\-\-block-' src/components/ src/renderers/ --include='*.css' --include='*.tsx'

# Find potential hardcoded colors
grep -rn '#[0-9a-fA-F]\{3,6\}' src/components/ src/renderers/ --include='*.css' --include='*.tsx'
grep -rn 'rgba\?(' src/components/ src/renderers/ --include='*.css' --include='*.tsx'
```

---

## Post-Audit Guidance

After producing the report:

1. **Fix CRITICAL issues first** — these are broken in a shipping theme
2. **Batch-fix HIGH issues** — deprecated token replacements can often be automated with sed
3. **Test in all 3 themes** — after fixing, verify by switching themes in the Settings → Appearance panel
4. **Update `tokens.css`** — if adding new tokens or fixing existing ones, ensure all 3 theme blocks are updated

### Known Discrepancies to Flag

These are known inconsistencies between the documentation and the codebase. Flag them when found — don't treat them as errors:

- **Toolbar height**: `tauterm-theme` skill says 36px, but `Toolbar.module.css` uses 40px
- **Rust ThemeEngine**: Backend `theme_engine.rs` defines 3 legacy themes (`neon-dark`, `ocean`, `sunset`) not matching frontend's `google-glow`, `obsidian`, `frosted` — low priority since the frontend handles theme switching independently

---

## Common Fix Patterns

### Fix: Hardcoded color → Token
```css
/* [错误] Before */  color: #888;
/* [正确] After  */  color: var(--text-secondary);
```

### Fix: Hardcoded background → Token
```css
/* [错误] Before */  background: rgba(0, 0, 0, 0.3);
/* [正确] After  */  background: var(--glass-input-bg);
```

### Fix: Missing global class
```tsx
// [错误] Before
<button className={styles.myBtn}>Click</button>

// [正确] After
<button className={`${styles.myBtn} liquid-glass-button`}>Click</button>
```

### Fix: Status tint → color-mix()
```css
/* [错误] Before */  background: rgba(255, 71, 87, 0.1);
/* [正确] After  */  background: color-mix(in srgb, var(--color-error) 10%, transparent);
```

### Fix: Floating element glass (can't use .liquid-glass)
```css
/* [正确] Self-contained glass for position:absolute/fixed elements */
.floatingPanel {
  position: absolute;
  background: var(--dialog-bg);
  backdrop-filter: blur(var(--glass-blur));
  -webkit-backdrop-filter: blur(var(--glass-blur));
  border: 1px solid var(--glass-border-default);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-glass), var(--dialog-shadow);
}
```
