---
name: tauterm-theme-review
description: >
  Audit and review TauTerm UI components for theme consistency across all 3 themes (google-glow, obsidian, frosted). Use this skill whenever the user asks to review, audit, check, or verify theme compatibility — including "review styles", "check theme consistency", "audit cross-theme", "verify frosted/light theme", "检查主题" (check theme), "审查样式" (review styles), "主题兼容性" (theme compatibility), or any request to ensure components look correct across themes. This is a QA/audit skill that examines EXISTING code and produces structured fix reports — distinct from tauterm-theme which enforces rules when writing NEW code. The light (frosted) theme is the most common source of hidden bugs since developers primarily work in dark mode.
license: MIT
metadata:
  author: tauterm
  version: "2.2"
---

# TauTerm 主题样式审查

> **语言偏好**：所有审查报告默认使用中文（简体中文）输出。

审查已有 UI 组件在 google-glow（炫彩流光）、obsidian（黑曜石）、frosted（白霜）三套主题下的兼容性。

## 与 `tauterm-theme` 的关系

`tauterm-theme` 是**编写**规范 —— 告诉开发者用什么全局类。本 skill 是**审查**工具 —— 检查已有代码是否遵循规范。

## 审查范围

`src/components/`、`src/renderers/`、`src/App.css`、`src/styles/`  
忽略：`node_modules/`、`dist/`、`src-tauri/`、测试文件

## 审查流程

### Step 1: 确定范围

询问用户审查范围：单个组件 / 组件组 / 布局 chrome / 全量项目。

### Step 2: 批量扫描

全量项目审计时，先跑批量 Grep 快速定位问题文件。**优先使用内置 Grep 工具**（ripgrep 引擎），注意 ripgrep 不支持 lookahead/lookbehind，需将复杂正则拆分为独立查询。并行发起所有扫描以提升效率。

**Scan A: 硬编码色值与废弃令牌**

```
grep '#[0-9a-fA-F]{6}' --glob='*.css' src/components/ src/renderers/
grep 'rgba?\(' --glob='*.css' src/components/ src/renderers/
grep '\-\-block-' --glob='*.{css,tsx}' src/
```

**Scan B: CSS Module 视觉属性（逐属性独立扫描）**

```
grep 'background:' --glob='*.module.css' src/components/ src/renderers/
grep 'box-shadow:' --glob='*.module.css' src/components/ src/renderers/
grep 'border-radius:' --glob='*.module.css' src/components/ src/renderers/
grep '  color:' --glob='*.module.css' src/components/ src/renderers/
```

**Scan C: 自建玻璃效果 🔑（CSS Module 中的 backdrop-filter 是高优先级信号）**

```
grep 'backdrop-filter:' --glob='*.module.css' src/components/ src/renderers/
```

> **关键判断**：如果 CSS Module 中同时出现 `position: absolute/fixed` 和 `backdrop-filter:`，该元素几乎必定应改用 `.liquid-glass-float` 全局类。

**Scan D: 全局类使用情况**

```
grep 'liquid-glass' --glob='*.tsx' src/components/
grep 'liquid-glass-float' --glob='*.tsx' src/components/
grep 'liquid-glass-toggle' --glob='*.tsx' src/components/
grep 'glass-overlay' --glob='*.tsx' src/
```

> **关键判断**：
> - 如果 `liquid-glass-float` 搜索结果为零，说明所有浮动元素都在手动实现玻璃效果——这是一个系统性违规，需要逐文件排查所有 `position: absolute/fixed` + `backdrop-filter:` 的 CSS Module。
> - 如果 `liquid-glass-toggle` 搜索结果为零，说明所有 toggle 开关都在组件中手动实现——需运行 Scan F 定位所有自定义 toggle 重实现。

**Scan E: emoji 使用**

```
grep '[\x{1F300}-\x{1F9FF}]' --glob='*.tsx' src/components/ | grep -v FileManager
```

**Scan F: 自定义 Toggle 重实现 🔑（CSS Module 中手写 checkbox-hack toggle 是高优先级信号）**

```
grep 'toggleTrack\|toggleLabel\|toggleSwitch\|switchTrack\|customToggle\|repeatCheck\|repeatLabel' --glob='*.module.css' src/components/ src/renderers/
grep 'toggleTrack\|toggleLabel\|toggleSwitch\|switchTrack\|customToggle\|repeatCheck\|repeatLabel' --glob='*.tsx' src/components/ src/renderers/
```

> **关键判断**：全局类 `.liquid-glass-toggle` 提供了完整的液态玻璃 toggle 开关样式（checkbox-hack 模式）。CSS Module 中任何 `position: absolute; opacity: 0` 隐藏原生 checkbox + 自定义 `div` 轨道 + 滑块的实现，都是对 `.liquid-glass-toggle` 的重复。这些 CSS 应全部删除，改为在 TSX 中使用 `className="liquid-glass-toggle"`，结构为 `<label className="liquid-glass-toggle"><input type="checkbox" /><div /></label>`。
>
> 如果组件需要在 toggle 旁显示文字标签，将文字放在 `div` 之后即可：`<label className="liquid-glass-toggle"><input /><div /><span>Label</span></label>`。如需组件级文字样式（color/font-size），可保留最小 CSS Module class 仅含文字属性，与 `liquid-glass-toggle` 组合使用。

### Step 3: 逐文件审查

对扫描命中的文件，按检查清单逐项审查。**优先处理以下高信号命中**：

1. **Scan C 命中的文件**（自建玻璃效果）→ 检查是否可替换为 `.liquid-glass-float`。读取对应 TSX 确认该元素的 `position` 属性
2. **Scan D 中 `liquid-glass-float` 结果为零或过少** → 说明浮动元素未使用全局类，逐个检查所有含 `backdrop-filter:` 的 CSS Module
3. **Scan D 中 `liquid-glass-toggle` 结果为零或过少** → 说明 toggle 开关在组件中手动实现，需运行 Scan F 定位所有自定义 toggle
4. **Scan F 命中的文件**（自定义 toggle 重实现）→ 检查 CSS Module 中是否包含 checkbox-hack 样式（隐藏原生 checkbox + 自定义轨道 + 滑块），这些应迁移到 `.liquid-glass-toggle`
5. **Scan B 命中的文件** → 检查视觉效果是否使用了 `var(--xxx)` 令牌（使用令牌可降级为 LOW，只报告未使用令牌的 hardcoded 值）

### Step 4: 产出报告

### Step 5: 建议修复

---

## 审查检查清单

### A: 硬编码值（CRITICAL）

| ID | 检查项 | 检测方式 |
|----|--------|---------|
| **A1** | 硬编码颜色 | grep `#[0-9a-fA-F]{3,8}` 和 `rgba?\(`。例外：Google 光球色（#4285F4, #EA4335, #FBBC05, #34A853）、`color: #fff` on holofoil buttons、已注释的 SVG data URI fill 色。状态色必须用 `var(--color-*)`，禁止硬编码 `#34d399` / `#eab308` |
| **A2** | CSS Module 含视觉属性 | grep CSS Module 中的 `background:` / `border:` / `box-shadow:` / `backdrop-filter:` / `border-radius:` / `color:`。例外：`border-left` / `border-bottom` 仅用于分隔线的情况 |

### B: 缺失全局类（HIGH）

| ID | 检查项 | 检测方式 | 修复 |
|----|--------|---------|------|
| **C1** | 布局 chrome 缺失 `.liquid-glass` | Toolbar / Sidebar / RightSidebar / StatusBar / TerminalView / SendBar / TransmissionPanel 的 className 必须含 `liquid-glass` | 添加 `liquid-glass`，CSS Module 仅保留布局属性 |
| **C2** | Input/Select/Textarea 缺失全局类 | `<input>`/`<textarea>` → `liquid-glass-input`；`<select>` → `liquid-glass-input liquid-glass-select` | 添加对应全局类 |
| **C3** | 玻璃按钮缺失 `.liquid-glass-button` | 次要按钮、图标按钮未使用 `liquid-glass-button` 或 `GlassButton` | 添加 `liquid-glass-button` class |
| **C4** | 主按钮缺失 `.liquid-primary-button` | 主 CTA 按钮（Connect/Send/Submit）未使用 `liquid-primary-button` 或 `GlassButton variant="primary"` | 添加 `liquid-primary-button` class |
| **C5** | 浮动元素缺失 `.liquid-glass-float` | `position: absolute/fixed` 元素在 CSS Module 中手动实现了玻璃效果（`backdrop-filter` + `background` + `border` + `box-shadow`），未使用 `.liquid-glass-float` 全局类 | CSS Module 删除视觉属性（仅保留 layout），TSX 添加 `liquid-glass-float` |
| **C6** | Toggle 开关缺失 `.liquid-glass-toggle` | CSS Module 中手写了 checkbox-hack toggle（隐藏原生 checkbox + 自定义 `div` 轨道 + 滑块 + 选中/禁用态），而未使用 `.liquid-glass-toggle` 全局类。检测方式：CSS Module 中出现 `position: absolute; opacity: 0` 用于隐藏 checkbox + `div` 元素的 `background`/`border-radius`/`box-shadow` + `::after` 滑块 | TSX 改为 `<label className="liquid-glass-toggle"><input type="checkbox" /><div /></label>`，CSS Module 删除所有 toggle 相关样式。如需内联文字，在 `div` 后加 `<span>` |

### D: Frosted（浅色）主题专项（CRITICAL）

浅色主题是最常见的隐藏 bug 来源——开发者主要在暗色模式下工作。

| ID | 检查项 | 检测方式 | 修复 |
|----|--------|---------|------|
| **G1** | 不可见边框 | `rgba(255,255,255,x)` 边框在 frosted 浅色背景 `#f8fafc` 上不可见 | 使用主题 token 定义边框，frosted 使用 `rgba(148,163,184,x)` |
| **G2** | 文字对比度不足 | frosted 下 `--text-muted` 对 `--bg-base` 需 ≥ 5.5:1 | 验证 WCAG AA 对比度 |
| **G3** | 暗色专属 `rgba(0,0,0,x)` | 在 frosted 浅色底上产生暗色斑块 | 使用 `var(--glass-*-bg)` token 或 `color-mix()` |
| **G4** | 硬编码 `mix-blend-mode: screen` | screen 在浅色底上效果不符预期 | 使用 `var(--bg-orb-blend)`（frosted 为 multiply） |
| **G5** | 硬编码遮罩透明度 | 遮罩 `rgba(0,0,0,x)` 不随主题变化 | 使用 `var(--overlay-bg)` |
| **G6** | 玻璃填充无阴影 | frosted 的 `--glass-fill` 无可见阴影 | 使用 `.liquid-glass` 或 `.liquid-glass-card`（含阴影） |

### E: 结构问题（MEDIUM）

| ID | 检查项 | 检测方式 | 修复 |
|----|--------|---------|------|
| **E1** | `.liquid-glass` 嵌套 | 一个 `.liquid-glass` 元素是另一个 `.liquid-glass` 的后代（导致 40px 阴影叠加 + 噪点三重叠加） | 内层改用 `.liquid-glass-card`。**修复前必须先确认 DOM 层次**：如果该组件是 RightSidebar/SendBar 等 glass 表面内的子面板，不应添加任何 glass 类 |
| **E2** | 浮动元素误用 `.liquid-glass` | `position: absolute/fixed` 元素使用了 `.liquid-glass`（会覆盖 position） | 改用 `.liquid-glass-float` |
| **E3** | 子面板误用 `.liquid-glass` | 渲染在已有 `liquid-glass` 祖先内部的子面板错误添加了 `liquid-glass`（如 RightSidebar → TransmissionPanel） | 直接移除 `liquid-glass`；子面板依赖父级 glass 表面，内部仅对独立区块使用 `liquid-glass-card` |

### F: 图标与 emoji（LOW）

| ID | 检查项 | 检测方式 | 修复 |
|----|--------|---------|------|
| **I1** | UI 控件使用 emoji | grep emoji Unicode 范围。例外：文件管理器文件类型（📁📄📂 等） | 替换为 `Icon` 组件 + PNG。提示词见 `src/assets/icons/prompts.md` |

---

## 报告格式

审查报告必须使用中文：

```markdown
# TauTerm 主题样式审查报告

**审查范围**：<组件或目录>
**日期**：<今天日期>
**检查主题**：google-glow（炫彩流光）、obsidian（黑曜石）、frosted（白霜）

## 摘要

| 严重程度 | 数量 |
|----------|------|
| [严重] CRITICAL | N |
| [高] HIGH     | N |
| [中] MEDIUM   | N |
| [低] LOW      | N |
| **合计**     | **N** |

## 按组件分类的发现

### <组件名>（`<文件路径>`）

| ID | 检查项 | 严重程度 | 位置 | 问题描述 | 修复建议 |
|----|--------|----------|------|----------|----------|
| A1 | 硬编码颜色 | CRITICAL | `file.module.css:42` | `color: #888` | 替换为 `var(--text-secondary)` |

## 批量修复命令

```bash
# 验证无残留自建玻璃效果（应返回零结果）
grep -rn 'backdrop-filter:' src/components/ --include='*.module.css'

# 验证 .liquid-glass-float 已正确使用（应返回所有浮动元素）
grep -rn 'liquid-glass-float' src/ --include='*.tsx'

# 验证布局 chrome 均有 .liquid-glass
grep -rn 'liquid-glass"' src/components/ --include='*.tsx'

# 验证无残留废弃令牌
grep -rn '\-\-block-' src/ --include='*.css' --include='*.tsx'

# 验证无残留硬编码色值
grep -rn '#[0-9a-fA-F]\{6\}' src/components/ src/renderers/ --include='*.css'

# 验证 .liquid-glass-toggle 已正确使用（应返回所有 toggle 开关）
grep -rn 'liquid-glass-toggle' src/ --include='*.tsx'

# 验证无残留自定义 toggle 重实现（应返回零结果）
grep -rn 'toggleTrack\|toggleLabel\|toggleSwitch\|switchTrack\|repeatCheck\|repeatLabel' src/ --include='*.module.css'
```

## 已知例外

### 允许的硬编码颜色

| 值 | 位置 | 原因 |
|---|------|------|
| `#4285F4, #EA4335, #FBBC05, #34A853` | `GoogleGlowBackground.tsx` | Google 品牌色 |
| `#fff` | `.liquid-primary-button`, `--text-on-accent` 引用 | 强调按钮文字始终白色 |
| SVG data URI fill 色 | `tokens.css` `--select-arrow` | 在各主题 tokens.css 注释中已注明 |

### 布局 chrome 表面

以下组件**必须**使用 `className={styles.xxx + ' liquid-glass'}`：
Toolbar、SessionSidebar、RightSidebar、StatusBar、TerminalView、SendBar。

> **注意**：此列表仅包含顶层布局 chrome——即直接渲染在应用布局根节点的独立表面。子面板（如 TransmissionPanel、ProtocolTool 等）渲染在 RightSidebar 的 `liquid-glass` 内部，**不应**重复添加 `liquid-glass`，否则造成嵌套玻璃效果（阴影叠加 + 噪点双重渲染）。

### 浮动元素（使用 `.liquid-glass-float`）

Toast、ContextMenu、SearchBar 下拉、SendBar 历史下拉、FilePreviewModal、FilePropertiesModal、InlinePrompt、各种弹窗/createPortal 元素。

### 透明按钮（不需要全局类）

TitleBar 窗口控制按钮、SessionSidebar 会话列表项、Settings 侧边栏导航项、ConnectDialog 模式卡、CommandPalette 结果项、GlassButton `variant="ghost"`、SendBar 历史项、FileManager 上下文菜单按钮和工具栏按钮。

### 微文本（8/9/10px 允许使用原始 px）

StatusBar（8/9/10px）、TransmissionPanel 分区标签/摘要/错误（9/10px）、CommandPalette 分组标题（9/10px）、SettingsPage 设置描述（10px）、TitleBar 标题（10px）、SessionSidebar meta（10px）、SendBar history（9/10px）。

### border-radius 例外

ProgressBar 动态 `height/2`、边缘接触元素 0px、圆形指示点 50%、SearchBar 部分圆角 `0 0 0 var(--radius-md)`。

### 装饰性图标字号例外

ConnectDialog `.modeIcon` 28px、Terminal `.emptyIcon` 32px — 需注释 `/* 装饰性 emoji/图标，不受字号令牌约束 */`。

---

## 审计前参考

读取以下文件以获取基础事实：

1. **`src/styles/tokens.css`** — 所有 CSS 自定义属性（~60 per theme）
2. **`src/styles/global.css`** — 全局类定义
3. **`.claude/skills/tauterm-theme/SKILL.md`** — 编写规范（核心规则 + 全局类目录）

### 快速 token 查询

```bash
# 查看特定主题的 token 值
grep -A 100 '\[data-theme="google-glow"\]' src/styles/tokens.css
grep -A 100 '\[data-theme="frosted"\]' src/styles/tokens.css

# 查找废弃 token（应返回零结果）
grep -rn '\-\-block-' src/components/ src/renderers/ --include='*.css' --include='*.tsx'

# 查找硬编码色值
grep -rn '#[0-9a-fA-F]\{3,6\}' src/components/ src/renderers/ --include='*.css' --include='*.tsx'
```

## 审查后指导

1. **先修 CRITICAL** — 这些在某个主题下直接不可用
2. **批量修 HIGH** — 缺失全局类通常可批量化
3. **三主题验证** — 在 Settings → Appearance 面板中切换验证
4. **更新 tokens.css** — 如需新增 token，确保三主题块同步更新

## 常见修复模式

### 硬编码色 → Token
```css
/* Before */  color: #888;
/* After  */  color: var(--text-secondary);
```

### 缺失全局类
```tsx
// Before
<button className={styles.myBtn}>Click</button>
// After
<button className={`${styles.myBtn} liquid-glass-button`}>Click</button>
```

### 状态底色 → color-mix()
```css
/* Before */  background: rgba(255, 71, 87, 0.1);
/* After  */  background: color-mix(in srgb, var(--color-error) 10%, transparent);
```

### 浮动元素 → `.liquid-glass-float`
```css
/* Before — 组件级 CSS 手写玻璃效果 */
.floatingPanel {
  position: absolute;
  background: var(--dialog-bg);
  backdrop-filter: blur(var(--glass-blur));
  border: 1px solid var(--glass-border-default);
  ...
}
/* After — 使用全局类 */
```
```tsx
<div className="liquid-glass-float" style={{ position: 'absolute', top: 40 }}>
```

### 自定义 Toggle → `.liquid-glass-toggle`
```tsx
// Before — 组件手动实现 checkbox-hack toggle
// TSX:
<label className={styles.toggleLabel}>
  <input type="checkbox" className={styles.hiddenCheck} checked={val} onChange={...} />
  <div className={styles.toggleTrack} />
</label>
// CSS Module:
.hiddenCheck { position: absolute; opacity: 0; width: 0; height: 0; }
.toggleTrack { width: 30px; height: 17px; background: var(--glass-input-bg); ... }
.toggleTrack::after { ... }  /* 滑块 */
.hiddenCheck:checked + .toggleTrack { ... }  /* 选中态 */
/* ... etc */

// After — 使用全局类
// TSX:
<label className="liquid-glass-toggle">
  <input type="checkbox" checked={val} onChange={...} />
  <div />
</label>
// CSS Module: 删除所有 toggle 相关样式
```
```tsx
// 如果 toggle 旁需要文字标签（如 ConnectDialog）
// Before:
<label className={styles.checkboxLabel}>
  <input type="checkbox" ... />
  <div className={styles.toggleTrack} />
  <span>Enable Feature</span>
</label>
// After — 保留最小 CSS Module class 用于文字样式：
<label className={`liquid-glass-toggle ${styles.checkboxLabel}`}>
  <input type="checkbox" ... />
  <div />
  <span>Enable Feature</span>
</label>
// CSS Module .checkboxLabel 仅含: color: var(--text-primary); font-size: var(--text-sm);
```

### emoji → Icon 组件
```tsx
// Before
<span>📡</span>
// After
<Icon name="antenna" size="sm" />
```
