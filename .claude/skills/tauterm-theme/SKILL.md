---
name: tauterm-theme
description: >
  Enforce TauTerm's Liquid Glass v3 theme system when writing UI code. Use this skill whenever the user asks to create, modify, or style any React component, CSS Module, or visual element in the TauTerm project. Covers all UI work — dialogs, panels, buttons, inputs, sidebars, toolbars, status bars, toggles, indicator dots, and any visual element. Also use when the user asks about theme tokens, CSS variables, dark/light mode, or wants to ensure a component looks correct across themes. This skill ensures zero hardcoded colors and full theme compatibility across google-glow, obsidian, and frosted themes.
license: MIT
metadata:
  author: tauterm
  version: "3.0"
---

# TauTerm Liquid Glass v3 主题系统

## 核心原则

所有视觉效果由**全局 CSS 类**管理（`src/styles/global.css`）。组件代码只负责**布局**，不定义视觉属性。

## 全局类目录

| 类名 | 用途 | 备注 |
|------|------|------|
| `.liquid-glass` | 完整玻璃面板 | 含 position: relative + 噪点纹理。用于布局 chrome 表面和弹窗。**不可**用于 absolute/fixed 元素 |
| `.liquid-glass-float` | 浮动玻璃面板 | 无 position 约束，无噪点纹理。用于 Toast、ContextMenu、SearchBar 下拉等 absolute/fixed 元素 |
| `.liquid-glass-card` | 内层卡片 | 基于 glass-fill + 16px 阴影。用于嵌套在 `.liquid-glass` 内的卡片 |
| `.liquid-glass-mini-card` | 微型卡片 | 基于 glass-fill + 6px 轻阴影。用于 <50px 的小型元素 |
| `.liquid-glass-button` | 次要玻璃按钮 | hover 上浮 + 阴影增强。用于 Cancel、Close、图标按钮 |
| `.liquid-glass-input` | 液态玻璃输入框 | 内凹底 + focus 蓝色辉光。用于所有 `<input>`、`<textarea>` |
| `.liquid-glass-select` | Select 下拉 | 必须与 `.liquid-glass-input` 组合使用 |
| `.liquid-glass-textarea` | 文本域 | 必须与 `.liquid-glass-input` 组合使用 |
| `.liquid-glass-toggle` | 切换开关 | 隐藏原生 checkbox 的液态玻璃样式 |
| `.liquid-glass-dot` | 状态指示点 | 配合 `.dot-success` / `.dot-error` / `.dot-warning` 变体 |
| `.liquid-primary-button` | 炫彩主动作按钮 | 全息渐变 + gradient-shift 动画 + 玻璃模糊 |
| `.glass-overlay` | 模态遮罩层 | fixed 定位 + flex 居中 |
| `.glow-orb` | 背景光球 | 用于 GoogleGlowBackground 中的 4 个流动光球 |

## 核心规则

### 1. 零硬编码颜色

所有 `color` / `background` / `border` / `box-shadow` / `backdrop-filter` 值必须来自 CSS 自定义属性（`var(--xxx)`）。

**已知例外：**
- `#fff` → `.liquid-primary-button` 文字色（始终白色）
- `#4285F4`, `#EA4335`, `#FBBC05`, `#34A853` → GoogleGlowBackground 光球色（固定设计特征）
- SVG data URI 的 fill 色（需在注释中注明值）

### 2. 视觉样式只用全局类

组件代码（CSS Module / inline style）**不得**定义以下属性：

> `background`  `border`  `border-radius`  `box-shadow`  `backdrop-filter`  `color`

这些属性全部由全局类接管。CSS Module 仅保留布局属性。

### 3. 布局不算样式

以下属性不属于"视觉样式"，可自由在组件代码中使用：

> `display`  `width`  `height`  `padding`  `gap`  `flex`  `overflow`  `position`  `align-items`

### 4. 无 emoji

除文件管理器文件类型图标（📁📄📂 等）外，**不使用 emoji** 作为 UI 元素。所有图标通过 `Icon` 组件从 `src/assets/icons/` 加载 PNG。图标提示词在 `src/assets/icons/prompts.md`。

### 5. 全局类不够用 → 新建全局类

如果需要全局类未覆盖的视觉效果（如新的控件类型），在 `src/styles/global.css` 中**新增全局类**——不写组件级视觉 CSS。

## Token 速查

**文字：**
- `--text-primary` — 正文、标题、活动项
- `--text-secondary` — 标签、描述、按钮文字（默认态）
- `--text-muted` — 占位符、时间戳、元数据、禁用态

**强调：**
- `--accent-primary` / `--accent-gradient` / `--accent-glow`
- `--text-on-accent` — 强调背景上的文字色（始终 #fff）

**状态色：**
- `--color-success` / `--color-error` / `--color-warning` / `--color-info`
- 状态着色背景：`color-mix(in srgb, var(--color-*) N%, transparent)`

## 组件模式

### 布局表面

```tsx
<div className={`${styles.toolbar} liquid-glass`}>
```
CSS Module 仅含布局属性（display、height、padding 等）。

### 浮动元素

```tsx
<div className="liquid-glass-float" style={{ position: 'absolute', top: 40 }}>
```

### 内层卡片

```tsx
<div className={`${styles.card} liquid-glass-card`}>
```

### 切换开关

```tsx
<label className="liquid-glass-toggle">
  <input type="checkbox" checked={enabled} onChange={...} />
  <div />
</label>
```

### 状态指示点

```tsx
<span className="liquid-glass-dot dot-success" />
<span className="liquid-glass-dot dot-error" />
```

### Select 下拉

```tsx
<select className={`${styles.mySelect} liquid-glass-input liquid-glass-select`}>
```
CSS Module 的 `.mySelect` 仅需定义 width 等布局属性。

### 炫彩主按钮

```tsx
<button className="liquid-primary-button">Connect</button>
// 或使用 GlassButton 组件: <GlassButton variant="primary">Connect</GlassButton>
```

## 布局栏约定

- **Toolbar** = 36px 固定高度 + `align-items: center`
- **StatusBar** = 26px 固定高度 + `align-items: center`
- **面板类**（侧边栏、传输面板、发送栏）= `height: 100%` / `flex: 1`

## 提交前检查

- [ ] 无硬编码色值（`grep -rn '#[0-9a-fA-F]\{3,6\}' src/components/ --include='*.css'`）
- [ ] 所有控件使用全局类（button→liquid-glass-button, input→liquid-glass-input, select→liquid-glass-input+liquid-glass-select）
- [ ] 无 emoji（文件类型除外）
- [ ] 三套主题切换正常（Settings → Appearance → google-glow / obsidian / frosted）
- [ ] CSS Module 未含视觉属性（`background`/`border`/`border-radius`/`box-shadow`/`backdrop-filter`/`color`）

## 源文件参考

- `src/styles/tokens.css` — 所有 CSS 自定义属性（Level 1 共享 + Level 2 主题）
- `src/styles/global.css` — 全局类定义
- `src/assets/icons/prompts.md` — 图标生成提示词（AI 生成 256×256 PNG）
- `src/context/ThemeContext.tsx` — 主题提供者
