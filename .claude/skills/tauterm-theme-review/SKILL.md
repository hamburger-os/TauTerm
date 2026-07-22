---
name: tauterm-theme-review
description: >
  Audit and review TauTerm UI components for theme consistency across all 3 themes (google-glow, obsidian, frosted). Use this skill whenever the user asks to review, audit, check, or verify theme compatibility — including "review styles", "check theme consistency", "audit cross-theme", "verify frosted/light theme", "检查主题" (check theme), "审查样式" (review styles), "主题兼容性" (theme compatibility), or any request to ensure components look correct across themes. This is a QA/audit skill that examines EXISTING code and produces structured fix reports — distinct from tauterm-theme which enforces rules when writing NEW code. The light (frosted) theme is the most common source of hidden bugs since developers primarily work in dark mode.
license: MIT
metadata:
  author: tauterm
  version: "2.0"
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

全量项目审计时，先跑批量 grep 快速定位问题文件：

```bash
# 硬编码色值
grep -rn '#[0-9a-fA-F]\{6\}' src/components/ src/renderers/ --include='*.css'
grep -rn 'rgba\?(' src/components/ src/renderers/ --include='*.css'

# 废弃令牌
grep -rn '\-\-block-' src/ --include='*.css' --include='*.tsx'

# CSS Module 含视觉属性
grep -rn 'background:' src/components/ src/renderers/ --include='*.module.css'
grep -rn 'border:' src/components/ src/renderers/ --include='*.module.css'
grep -rn 'box-shadow:' src/components/ src/renderers/ --include='*.module.css'
grep -rn 'border-radius:' src/components/ src/renderers/ --include='*.module.css'
grep -rn 'backdrop-filter:' src/components/ src/renderers/ --include='*.module.css'
grep -rn '\bcolor:' src/components/ src/renderers/ --include='*.module.css' | grep -v '//'

# 嵌套 .liquid-glass
grep -rn 'liquid-glass"' src/components/ --include='*.tsx'

# emoji 使用
grep -rnP '[\x{1F300}-\x{1F9FF}]' src/components/ --include='*.tsx' | grep -v FileManager
```

### Step 3: 逐文件审查

对扫描命中的文件，按检查清单逐项审查。

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
| **E1** | `.liquid-glass` 嵌套 | 一个 `.liquid-glass` 元素是另一个 `.liquid-glass` 的后代（导致 40px 阴影叠加 + 噪点三重叠加） | 内层改用 `.liquid-glass-card` |
| **E2** | 浮动元素误用 `.liquid-glass` | `position: absolute/fixed` 元素使用了 `.liquid-glass`（会覆盖 position） | 改用 `.liquid-glass-float` |

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
# 验证无残留废弃令牌
grep -rn '\-\-block-' src/ --include='*.css' --include='*.tsx'
# 验证 docs/ 无引用
grep -rn 'docs/theme-guide' .claude/
```
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
Toolbar、SessionSidebar、RightSidebar、StatusBar、TerminalView、SendBar、AutoReplyPanel、ScriptEditor、TransmissionPanel。

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

### emoji → Icon 组件
```tsx
// Before
<span>📡</span>
// After
<Icon name="antenna" size="sm" />
```
