---
name: tauterm-theme
description: "Single source of truth for TauTerm Liquid Glass UI, theme tokens, material layering, visual performance tiers, and rendering-performance rules. Use for any React/CSS UI creation or modification, theme work, visual review/fixes, animation, glass/backdrop effects, or appearance/performance settings."
license: MIT
metadata:
  author: tauterm
  version: "4.0"
---

# TauTerm Liquid Glass v4 — 唯一主题规范源

> **SSOT**：本文件是 TauTerm 视觉材质、主题兼容和 UI 渲染性能规则的唯一规范源。  
> `tauterm-theme-review` 只能描述“如何审查”，不得复制或重新定义本文件中的规则。

## 目标

TauTerm 的 Liquid Glass 不是“大面积毛玻璃”。设计目标是：

- **清透**：内容稳定清晰，背景只提供氛围。
- **边缘光学感**：通过 specular/rim light、薄边框、轻阴影表达玻璃厚度。
- **克制的动态**：液态反馈集中在活动控件与小面积 chrome，不让整个内容层持续重绘。
- **老设备可用**：默认 Balanced 必须避免大面积实时 blur；Compatibility 在没有高性能 GPU 时仍保持完整的层次与配色。
- **三主题一致**：google-glow / obsidian / frosted 使用同一材质语义，只替换 token。

## 1. 材质分层（必须先判断 surface）

### A. Chrome — `.liquid-glass`

用于 Toolbar、SessionSidebar/RightSidebar 外壳、StatusBar、Dialog/Settings 主框架和小面积固定工具面。

- 可使用轻量 `backdrop-filter`。
- blur 强度由 `--glass-blur-chrome` 和 `data-performance` 控制。
- 使用 `--glass-specular-fill` + `--glass-fill` + rim/shadow。
- **禁止**用于终端、文件列表、数据表、图表主体等大面积内容层。

### B. Content — `.liquid-glass-content`

用于 Terminal pane、File browser/data view、Network/TFTP/iperf/TRDP 主内容、可扩展 SendBar 主体、空 Pane/断开占位。

- **不得使用 `backdrop-filter`**。
- 使用稳定半透明 `--content-surface`。
- 活跃/非活跃分屏分别用 `.liquid-glass-content-active` / `.liquid-glass-content-inactive`。
- 内容面应该“透一点”，但不能让彩色背景直接抢过文本和数据。

### C. Accent — `.liquid-glass-accent`

用于 command/search trigger、active pill、segmented control、小型 hover/selected control。

- 默认不常驻 backdrop blur。
- 通过 tint、specular edge、rim light、短交互位移表达液态感。

### D. Float — `.liquid-glass-float`

用于 ContextMenu、Toast、Search dropdown、Popover 等 absolute/fixed/createPortal 浮层。

可使用 `--glass-blur-float`，Compatibility 自动禁用 backdrop blur。

### E. Nested cards

使用 `.liquid-glass-card` / `.liquid-glass-mini-card` / `.liquid-glass-status-card`。嵌套在 chrome/content 内部时用 card，而不是再套一层 `.liquid-glass`。

## 2. 性能档（Appearance → Visual Performance）

`ThemeContext` 在 `<html>` 写入 `data-performance`：

| 模式 | 语义 | 要求 |
|---|---|---|
| `quality` | 清透优先 | 可提高**小面积** chrome/float 的 blur；仍禁止大面积实时 blur |
| `balanced` | 默认/推荐 | 10–12px 级 chrome/float sampling；内容面无 backdrop blur |
| `compat` | 老电脑/软件渲染/远程桌面 | backdrop blur = 0；环境光静态化并减少数量；保留配色、边缘、层次 |

**禁止通过 GPU 型号猜测自动切档。** GPU/WebView/驱动能力很难可靠推断。默认 Balanced，用户可明确选择 Compat。

## 3. 动态与后台占用

`ThemeContext` 在 `<html>` 写入 `data-motion`：

- `full`：正常
- `reduced`：系统 `prefers-reduced-motion`
- `paused`：窗口隐藏或失焦

规则：

1. 环境背景只能使用 **transform-only** 动画。
2. 窗口隐藏/失焦时 CSS animation 必须暂停。
3. reduced-motion 下装饰动画必须停止或缩短。
4. Loading/状态语义动画可以保留，但不得成为大面积持续合成源。

## 4. 背景流光规则

`GoogleGlowBackground` 是品牌氛围层，不是内容层。

必须：

- 使用预柔化 `radial-gradient`。
- 只动画 `transform`。
- Balanced/Quality 可保留四色。
- Compat 静态化并减少光团。

禁止：

- 大面积 `filter: blur(...)`。
- 100px+ 实时 blur。
- 动画 `border-radius` / clip-path 做持续 morph。
- 全屏 `mix-blend-mode`。
- 常驻 `will-change`。
- 背景动画导致内容层再次 backdrop capture。

Google 四色是允许的固定品牌色：`#4285F4` / `#EA4335` / `#FBBC05` / `#34A853`。

## 5. Backdrop-filter 红线

`backdrop-filter` / `-webkit-backdrop-filter` 只能由 `src/styles/global.css` 中的全局材质类实现。

**CSS Module 中出现 backdrop-filter 默认视为 HIGH/CRITICAL 问题。**

尤其禁止：

- Terminal / xterm 外壳
- 大面积 SplitView pane
- FileManager 主体
- 图表/日志/数据表主体
- 可展开到大面积的 SendBar
- glass 内再嵌 glass

Compatibility 必须能把所有允许的 backdrop blur 统一关掉。

## 6. CSS 与 Token 规则

### 颜色

业务组件不得写新的硬编码颜色。使用 `--text-*`、`--accent-*`、`--color-*`、`--glass-*`、`--content-*`。

允许例外：

- Google 四色环境光
- 永远为白色的 `#fff` on-accent 文本
- 已有、注明原因的 SVG data URI 色值

### CSS Module

CSS Module **可以**定义组件局部视觉状态，但必须满足：

- 颜色/阴影/边框来自 token。
- 不重建 glass/backdrop 材质。
- 不复制全局 button/input/toggle 材质。
- 状态差异（active/hover/selected）尽量只改 token 化的 background/border/opacity。

### 全局材质

涉及 surface、button、input、toggle、float 的共享视觉优先复用全局类。

## 7. 动画与 transition 性能规则

禁止：

- `transition: all`
- 大面积 filter 动画
- 持续背景渐变 animation
- 常驻 `will-change`
- 为了“GPU 加速”无条件添加 `translateZ(0)` / `backface-visibility:hidden`

推荐只声明会变化的属性：

```css
transition:
  background-color var(--transition-fast),
  border-color var(--transition-fast),
  box-shadow var(--transition-fast),
  transform var(--transition-fast);
```

主按钮全息渐变通过 hover 时 `background-position` 过渡，不做 idle infinite animation。

## 8. Terminal / SplitView 特殊规则

Terminal 是性能最高优先级内容面。

- xterm 背景可以 transparent，但透明目标应该是 `.liquid-glass-content`，不是最底层动态背景。
- 非活动 terminal 保留实例/scrollback，但淡出后必须 `visibility:hidden`，避免继续 paint/compositing。
- 不因切 Tab dispose/recreate xterm。
- SplitView 仅对可见 pane 绘制 active/inactive content surface。
- 分屏几何 resize 可以短 transition；拖动过程中不得引入 blur 动画。
- 不对隐藏 xterm 强行使用未经验证的 `content-visibility`，避免尺寸/fit 回归。

## 9. Sidebar 信息密度

Sidebar 外壳是 chrome；session item 默认应扁平：

- 默认项：透明边框/无卡片阴影
- hover：轻 tint + 轻 border
- active：更明确 tint + rim
- 不让每一项都成为独立“玻璃卡”

## 10. 控件类

| 类 | 用途 |
|---|---|
| `.liquid-glass-button` | 次要按钮 |
| `.liquid-glass-ghost-button` | chrome 透明图标按钮 |
| `.liquid-primary-button` | 主 CTA；不得 idle 无限动画/常驻 blur |
| `.liquid-glass-input` | input |
| `.liquid-glass-select` | 与 input 组合 |
| `.liquid-glass-textarea` | 与 input 组合 |
| `.liquid-glass-toggle` | toggle |
| `.liquid-glass-dot` | 状态点 |
| `.glass-overlay` | 模态遮罩 |

`GlassPanel` 通过 `surface="chrome" | "content" | "accent"` 表达材质语义；大面积内容必须使用 `surface="content"`。

## 11. 图标与 emoji

除 `src/components/FileManager/entryIcon.ts` 已定义的文件类型 emoji 外，UI 不使用 emoji 充当控件图标。使用 `Icon` + `src/assets/icons/`。

## 12. 三主题要求

任何新增材质 token 必须同时在 google-glow、obsidian、frosted 三个主题中定义。

Frosted 必须特别检查：

- 边框是否在浅底可见
- muted text 对比度
- 是否存在只适合暗色的黑块/白边
- content surface 是否足够稳定而非“白雾”

## 13. 提交前检查

至少执行：

```bash
rg 'backdrop-filter' src/components src/renderers --glob '*.module.css'
rg 'transition:\s*all' src --glob '*.css'
rg 'filter:\s*blur|mix-blend-mode|will-change' src --glob '*.css' --glob '*.tsx'
rg 'liquid-glass-content|liquid-glass-accent|liquid-glass-float|liquid-glass' src --glob '*.tsx'
rg 'data-performance|glass-blur-chrome|content-surface' src
npm run build
```

视觉验证至少覆盖：

1. google-glow / obsidian / frosted
2. quality / balanced / compat
3. 1 pane 与 4 pane
4. 多个后台 xterm
5. 窗口失焦后 GPU/动画是否停止
6. 大量终端输出时交互是否保持流畅

## 实现源文件

- `src/styles/tokens.css` — 主题与性能 token
- `src/styles/global.css` — 全局材质类、环境光与降级规则
- `src/context/ThemeContext.tsx` — theme / performance / motion 状态
- `src/components/Layout/GoogleGlowBackground.tsx` — 环境光层
- `src/components/Terminal/TerminalView.tsx` — xterm 实例与可见性策略
- `src/components/Layout/SplitView.tsx` — pane 内容材质分配
