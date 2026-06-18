# Liquid Glass v3 — UI 全面重构设计文档

> **状态**: 设计完成，待实施  
> **日期**: 2026-06-19  
> **目标**: 将 TauTerm UI 从静态玻璃拟态升级为"液态毛玻璃 + 动态炫彩流光"的前卫视觉体验

---

## 1. 设计愿景

结合 **Liquid Glass（液态毛玻璃）** 与 **Google 炫彩流光（Gemini 式动态光效）**，打造极具视觉冲击力的下一代终端用户界面。

### 核心原则

- **流动而不是静止** — 光效随交互而生，空闲时优雅消退
- **Chrome 是画布，终端是焦点** — Shell/边框/面板承载视觉冲击，终端区域保持干净可读
- **形态即语言** — 圆角、边框、阴影的层次韵律传递"液态表面张力"
- **三种气质** — 炫彩（前卫）、黑曜石（稳重）、水晶（通透）

### 影响范围

- **Shell/Chrome 层**全量升级：Toolbar、Sidebar、StatusBar、面板、按钮、输入框、分割线
- **终端视口**保持干净：使用 `disabled` 模式关闭流动效果
- 旧有 3 主题（neon-dark / ocean / sunset）被 3 新主题替换

---

## 2. 架构总览

### 2.1 渲染层级（从后到前）

```
第 1 层 — 应用背景
  CSS 渐变，极缓慢呼吸动画（6-8s 周期）
  深色主题: 深色底 + 微明暗变化
  亮色主题: 浅灰白底 + 微温变化

第 2 层 — SVG 流动纹理（NEW）
  feTurbulence 分形噪声 + feColorMatrix 色彩映射
  mix-blend-mode 叠加到玻璃上
  framer-motion 驱动 baseFrequency / gradientOrigin 参数

第 3 层 — 玻璃表面（升级）
  backdrop-filter: blur() + 主题玻璃色
  从静态 CSS 变量升级为 framer-motion 动态值

第 4 层 — 边框流光（NEW）
  conic-gradient 旋转色带 + mask 裁剪
  仅交互时可见，由动画状态机控制 opacity

第 5 层 — 内容层
  文字、图标、终端实例、按钮内容
  不受光效影响
```

### 2.2 技术路线

**路线：混合式 — CSS 玻璃骨架 + SVG 滤镜纹理 + framer-motion 编排**

- CSS: backdrop-filter 玻璃基座 + CSS 自定义属性主题系统
- SVG: feTurbulence 分形噪声产生有机流动纹理，feColorMatrix 按主题映射色彩
- framer-motion: 编排动画状态机、鼠标追踪、交互触发

### 2.3 新增/修改文件清单

```
src/
├── styles/
│   ├── tokens.css          → tokens-v3.css     【重写】
│   ├── flow-filters.css                        【新增】
│   └── global.css                              【修改】
├── hooks/
│   └── useFlowInteraction.ts                   【新增】
├── components/
│   └── common/
│       ├── FlowSurface.tsx                     【新增】核心流动包装器
│       ├── BorderGlow.tsx                      【新增】边框流光
│       ├── GlassPanel.tsx                      【修改】集成 FlowSurface
│       ├── GlassButton.tsx                     【修改】流动 hover + 涟漪
│       ├── GlassInput.tsx                      【修改】流动 focus
│       ├── GlassPanel.module.css               【修改】
│       └── GlassButton.module.css              【修改】
│   └── Layout/
│       ├── Toolbar.tsx / .module.css           【修改】
│       ├── SessionSidebar.tsx / .module.css    【修改】
│       ├── StatusBar.tsx / .module.css         【修改】
│       └── ResizeHandle.tsx / .module.css      【修改】
├── context/
│   └── ThemeContext.tsx                        【修改】新主题 ID
└── App.css                                     【修改】
```

---

## 3. CSS 令牌体系 v3

令牌从 v2 的扁平一维结构升级为 **五层分层结构**，每层对应渲染层级。

### 3.1 第 0 层 — 基础常量（跨主题共享）

```css
:root {
  /* 圆角 */
  --radius-ui-sm: 6px;
  --radius-ui-md: 10px;
  --radius-ui-lg: 14px;
  --radius-panel: 16px;
  --radius-window: 20px;
  --radius-full: 9999px;

  /* 间距 */
  --spacing-xs: 4px;
  --spacing-sm: 8px;
  --spacing-md: 12px;
  --spacing-lg: 16px;
  --spacing-xl: 24px;
  --spacing-2xl: 32px;

  /* 字体 */
  --font-ui: "Inter", -apple-system, BlinkMacSystemFont, "Segoe UI",
    "Microsoft YaHei", "PingFang SC", "Noto Sans SC", sans-serif;
  --font-mono: "JetBrains Mono", "Cascadia Code", "Fira Code",
    "Consolas", "Courier New", monospace;

  /* 字号 */
  --text-xs: 0.7rem;
  --text-sm: 0.78rem;
  --text-base: 0.85rem;
  --text-md: 0.95rem;
  --text-lg: 1.1rem;
  --text-xl: 1.25rem;

  /* 过渡 */
  --transition-fast: 150ms ease;
  --transition-normal: 300ms ease;
  --transition-slow: 500ms ease;
  --transition-dissipate: 2s ease-out;

  /* Z-index */
  --z-surface: 1;
  --z-panel: 10;
  --z-sidebar: 10;
  --z-overlay: 30;
  --z-toast: 50;

  /* 阴影深度系统 */
  --shadow-1: 0 2px 8px rgba(0, 0, 0, 0.3);
  --shadow-2: 0 8px 24px rgba(0, 0, 0, 0.5);
  --shadow-3: 0 16px 48px rgba(0, 0, 0, 0.7);
}
```

### 3.2 主题色彩定义

#### 🌈 炫彩 Prismatic `[data-theme="prismatic"]`

```css
[data-theme="prismatic"] {
  /* 第 1 层 — 背景 */
  --bg-base: #060612;
  --bg-secondary: #0a0a1e;
  --bg-gradient: linear-gradient(135deg, #060612 0%, #0a0a1e 40%, #0c0c24 100%);

  /* 第 2 层 — 流动纹理 */
  --flow-color-1: #6366f1;  /* 靛蓝 */
  --flow-color-2: #a855f7;  /* 紫罗兰 */
  --flow-color-3: #ec4899;  /* 粉红 */
  --flow-color-4: #06b6d4;  /* 青色 */
  --flow-blend-mode: screen;
  --flow-idle-opacity: 0;
  --flow-active-opacity: 0.35;
  --flow-idle-frequency: 0.0005;
  --flow-aware-frequency: 0.003;
  --flow-active-frequency: 0.015;

  /* 第 3 层 — 玻璃表面 */
  --glass-bg: rgba(99, 102, 241, 0.03);
  --glass-bg-hover: rgba(99, 102, 241, 0.06);
  --glass-bg-active: rgba(99, 102, 241, 0.10);
  --glass-border: rgba(99, 102, 241, 0.10);
  --glass-border-hover: rgba(99, 102, 241, 0.25);
  --glass-border-focus: rgba(99, 102, 241, 0.45);
  --glass-blur: 16px;
  --glass-radius: 14px;

  /* 第 4 层 — 边框流光 */
  --border-glow-color: rgba(139, 92, 246, 0.35);
  --border-glow-spread: 12px;
  --border-flow-speed: 3s;

  /* 强调色 */
  --accent-primary: #818cf8;
  --accent-secondary: #6366f1;
  --accent-glow: rgba(139, 92, 246, 0.35);
  --accent-gradient: linear-gradient(135deg, #6366f1 0%, #a855f7 100%);

  /* 文字 */
  --text-primary: #e0e0ff;
  --text-secondary: #8888bb;
  --text-muted: #505080;
  --text-accent: #818cf8;

  /* 状态颜色 */
  --color-success: #34d399;
  --color-error: #ff4757;
  --color-warning: #ffa502;
  --color-info: #06b6d4;

  /* 阴影（暗底增强） */
  --shadow-glass: 0 8px 32px rgba(0, 0, 0, 0.6);
  --shadow-glow: 0 0 20px rgba(139, 92, 246, 0.2);

  /* 形态 */
  --border-width-panel: 1.5px;
  --border-width-control: 1px;
}
```

#### 🖤 液态黑曜石 Obsidian `[data-theme="obsidian"]`

```css
[data-theme="obsidian"] {
  /* 第 1 层 — 背景 */
  --bg-base: #080808;
  --bg-secondary: #0a0a0a;
  --bg-gradient: linear-gradient(135deg, #080808 0%, #0a0a0a 40%, #0d0d0d 100%);

  /* 第 2 层 — 流动纹理 */
  --flow-color-1: #b8860b;  /* 暗金 */
  --flow-color-2: #cd853f;  /* 铜色 */
  --flow-color-3: #8b6914;  /* 古铜 */
  --flow-color-4: #d4a574;  /* 暖沙 */
  --flow-blend-mode: soft-light;
  --flow-idle-opacity: 0;
  --flow-active-opacity: 0.25;
  --flow-idle-frequency: 0.0003;
  --flow-aware-frequency: 0.002;
  --flow-active-frequency: 0.008;

  /* 第 3 层 — 玻璃表面 */
  --glass-bg: rgba(184, 134, 11, 0.03);
  --glass-bg-hover: rgba(184, 134, 11, 0.05);
  --glass-bg-active: rgba(184, 134, 11, 0.08);
  --glass-border: rgba(184, 134, 11, 0.08);
  --glass-border-hover: rgba(184, 134, 11, 0.20);
  --glass-border-focus: rgba(184, 134, 11, 0.35);
  --glass-blur: 20px;
  --glass-radius: 16px;

  /* 第 4 层 — 边框流光 */
  --border-glow-color: rgba(184, 134, 11, 0.25);
  --border-glow-spread: 8px;
  --border-flow-speed: 5s;

  /* 强调色 */
  --accent-primary: #cd853f;
  --accent-secondary: #b8860b;
  --accent-glow: rgba(184, 134, 11, 0.25);
  --accent-gradient: linear-gradient(135deg, #b8860b 0%, #cd853f 100%);

  /* 文字 */
  --text-primary: #e8d6c0;
  --text-secondary: #998877;
  --text-muted: #605040;
  --text-accent: #cd853f;

  /* 状态颜色 */
  --color-success: #4ade80;
  --color-error: #ff4757;
  --color-warning: #fbbf24;
  --color-info: #cd853f;

  /* 阴影（最强厚重感） */
  --shadow-glass: 0 12px 40px rgba(0, 0, 0, 0.8);
  --shadow-glow: 0 0 16px rgba(184, 134, 11, 0.15);

  /* 形态（最厚重） */
  --border-width-panel: 2px;
  --border-width-control: 1px;
}
```

#### 💎 液态水晶 Crystal `[data-theme="crystal"]`

```css
[data-theme="crystal"] {
  /* 第 1 层 — 背景 */
  --bg-base: #f0f0f3;
  --bg-secondary: #f5f5f8;
  --bg-gradient: linear-gradient(135deg, #f0f0f3 0%, #f5f5f8 40%, #fafafc 100%);

  /* 第 2 层 — 流动纹理 */
  --flow-color-1: #e8d5f5;  /* 淡紫 */
  --flow-color-2: #d5e8f5;  /* 淡蓝 */
  --flow-color-3: #f5d5e0;  /* 淡粉 */
  --flow-color-4: #d5f5e8;  /* 淡薄荷 */
  --flow-blend-mode: overlay;
  --flow-idle-opacity: 0;
  --flow-active-opacity: 0.20;
  --flow-idle-frequency: 0;
  --flow-aware-frequency: 0.003;
  --flow-active-frequency: 0.010;

  /* 第 3 层 — 玻璃表面 */
  --glass-bg: rgba(255, 255, 255, 0.40);
  --glass-bg-hover: rgba(255, 255, 255, 0.55);
  --glass-bg-active: rgba(255, 255, 255, 0.70);
  --glass-border: rgba(0, 0, 0, 0.06);
  --glass-border-hover: rgba(0, 0, 0, 0.12);
  --glass-border-focus: rgba(100, 100, 180, 0.30);
  --glass-blur: 12px;
  --glass-radius: 12px;

  /* 第 4 层 — 边框流光 */
  --border-glow-color: rgba(180, 180, 200, 0.15);
  --border-glow-spread: 6px;
  --border-flow-speed: 4s;

  /* 强调色 */
  --accent-primary: #8888cc;
  --accent-secondary: #7777bb;
  --accent-glow: rgba(130, 130, 180, 0.20);
  --accent-gradient: linear-gradient(135deg, #9999dd 0%, #7777cc 100%);

  /* 文字 */
  --text-primary: #1a1a2e;
  --text-secondary: #555566;
  --text-muted: #9999aa;
  --text-accent: #7777cc;

  /* 状态颜色 */
  --color-success: #22c55e;
  --color-error: #ef4444;
  --color-warning: #eab308;
  --color-info: #8888cc;

  /* 阴影（最轻） */
  --shadow-glass: 0 4px 16px rgba(0, 0, 0, 0.06);
  --shadow-glow: 0 0 12px rgba(130, 130, 180, 0.10);

  /* 形态（最轻薄） */
  --border-width-panel: 1px;
  --border-width-control: 1px;
}
```

---

## 4. 动画状态机

### 4.1 四态循环

```
                    mouse enters panel
    ┌──────────────────────────────────────┐
    │                                      │
    ▼                                      │
  IDLE ──────► AWARE ──────► FLOWING ──────► DISSIPATING
   ▲             │              │                │
   │             │              │                │
   └─────────────┴──────────────┴────────────────┘
```

### 4.2 各状态属性映射

| 属性 | IDLE | AWARE | FLOWING | DISSIPATE |
|------|------|-------|---------|-----------|
| `feTurbulence.baseFrequency` | `idleFreq` | `awareFreq` | `activeFreq` | → `idleFreq` |
| 渐变原点 `cx, cy` | 50%, 50% | 光标相对位置×0.3 | 光标实时位置 | → 50%, 50% |
| 玻璃 `background` | `--glass-bg` | `--glass-bg-hover` | `--glass-bg-active` | → `--glass-bg` |
| 边框流光 `opacity` | 0 | 0.3 | 1.0 | → 0 |
| 边框色带 `rotation` | 0deg | 缓慢旋转 | 1-3s 周期旋转 | → 0deg |
| 流动纹理 `opacity` | `idleOpacity` | 渐入 | `activeOpacity` | → `idleOpacity` |

### 4.3 状态转换参数

| 转换 | 延迟 | 缓动 |
|------|------|------|
| IDLE → AWARE | 立即 (0ms) | ease-out |
| AWARE → FLOWING | 100ms (防止误触) | ease-out |
| FLOWING → DISSIPATE | 立即 (0ms) | - |
| DISSIPATE → IDLE | ~2s | ease-out |
| AWARE → IDLE | ~1s | ease-out |

### 4.4 光标速度感应

`useFlowInteraction` hook 追踪光标速度：

- **快速划过** → 低湍流 + 大渐变拖尾 → "光在鼠标后面追"
- **慢速/静止** → 高湍流 + 渐变原位微动 → "光在指尖打转"

### 4.5 空闲呼吸

IDLE 状态下全局背景有一个极缓慢的呼吸动画，让应用在无交互时仍然"活着"：

```css
@keyframes bg-breathe {
  0%, 100% { opacity: 0.85; }
  50%      { opacity: 1.0; }
}
/* duration: 7s ease-in-out, infinite */
```

### 4.6 跨面板协调

同一时刻只有一个面板处于 FLOWING 状态。快速划过多个面板时产生拖尾效果：

```
面板A: FLOWING → DISSIPATING → IDLE
面板B:         IDLE → AWARE → FLOWING → DISSIPATING
面板C:                           IDLE → AWARE → IDLE

视觉: 光标路径留下逐渐消散的"光尾"
```

---

## 5. 形态与几何语言

### 5.1 圆角分层体系

```
层级              半径            用途
─────────────────────────────────────────
窗体外框          20px            应用窗口四角
大型面板          16px            侧栏、设置页、连接对话框
中型面板          12px            传输面板、Toast 通知
小型控件          10px            按钮、输入框、下拉框
内联元素          6px             状态标签、徽章、小指示器
终端视口          4px             xterm 内边距边缘
胶囊元素          9999px          开关、标签页、进度条

原则: 从外到内，圆角递减；同层元素，曲率一致
```

### 5.2 嵌套曲率比

```
外层面板 radius = 16px
内层面板 radius = 12px
面板间距 = 16px

曲率比  外层 : 内层 : 间距
      ≈  4  :  3  :  4
```

### 5.3 边框分层策略

| 层级 | 样式 | 用途 |
|------|------|------|
| 面板外框 | `var(--border-width-panel) solid` + 1px 渐变叠加 | 立体双层边框 |
| 控件边框 | `var(--border-width-control) solid` + focus glow | 简洁清晰 |
| 分割线 | 0.5px solid `--glass-border` | 微妙视觉分割 |
| 流光边框 | 1px conic-gradient 旋转 (BorderGlow 组件) | 仅交互时显现 |

### 5.4 阴影深度系统

```
z-0 (终端区)  → 无阴影
z-1 (状态栏/工具栏/侧栏/发送栏/分割线) → shadow-1 (0 2px 8px)
z-2 (面板/对话框/Toast/传输面板/搜索栏) → shadow-2 (0 8px 24px)
z-3 (命令面板/下拉菜单)                 → shadow-3 (0 16px 48px)
z-4 (拖拽覆盖层)                        → shadow-3 + backdrop blur

亮色主题使用更轻的 shadow 值
```

### 5.5 按钮液态形态

```
类型         默认形状   hover 变化
────────────────────────────────────────
胶囊按钮     9999px    scale(1.02) + 光晕扩展
圆角按钮     10px      → 12px（液滴被"按压"）
图标按钮     6px       → 8px + 背景光晕
Ghost 按钮   6px       → 8px + 极淡背景浮现

按压 (mousedown): scale(0.96-0.97)
释放: 弹回 + borderRadius 恢复
```

### 5.6 主题差异化形态

| 属性 | Prismatic (炫彩) | Obsidian (黑曜石) | Crystal (水晶) |
|------|:---:|:---:|:---:|
| 面板圆角 | 14px | 16px | 12px |
| 按钮圆角 | 10px | 8px | 8px |
| 面板边框宽度 | 1.5px | 2px | 1px |
| 玻璃模糊 | 16px | 20px | 12px |
| 阴影深度 | 中 | 最深 | 最轻 |
| 整体气质 | 灵动前卫 | 厚重沉稳 | 轻盈通透 |

---

## 6. 核心组件设计

### 6.1 FlowSurface.tsx（新增）

流动玻璃核心包装器。内部结构：

```
<div style={{ position: "relative" }}>
  ┌─ SVG 纹理层 (absolute, inset:0, z-index:0)
  │   <filter id="flow-{id}">
  │     <feTurbulence type="fractalNoise"
  │       baseFrequency={motionFreq} numOctaves="3" />
  │     <feColorMatrix values={themeFlowMatrix} />
  │     <feBlend mode={flowBlendMode} />
  │   </filter>
  │   <rect fill="url(#flow-{id})" />
  ├─ 玻璃层 (relative, z-index:1)
  │   backdrop-filter: blur(--glass-blur)
  │   background: motionValue(--glass-bg → active)
  │   border-radius: --glass-radius
  ├─ 边框流 (absolute, overlay, z-index:2)
  │   BorderGlow 组件
  ├─ 内容层 (relative, z-index:3)
      {children}
```

**Props:**

| Prop | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `variant` | `"panel" \| "toolbar" \| "button" \| "handle"` | `"panel"` | 变体 |
| `disabled` | `boolean` | `false` | 关闭流动（终端区使用） |
| `children` | `ReactNode` | - | 内容 |
| `className` | `string` | - | 额外类名 |

**变体行为差异:**

| 变体 | 鼠标追踪 | 湍流强度 | 边框流 |
|------|---------|---------|--------|
| `panel` | 全局 | 中 | 四边 |
| `toolbar` | 水平方向 | 低 | 仅底/顶边 |
| `button` | 按钮内 | 高 | 四边 + 涟漪 |
| `handle` | 拖拽方向 | 中 | 拖拽轨迹 |

### 6.2 BorderGlow.tsx（新增）

边框流动光带组件。

```
实现: ::after 伪元素 + conic-gradient + mask

conic-gradient(
  from var(--flow-angle),
  --flow-color-1, --flow-color-2,
  --flow-color-3, --flow-color-4,
  --flow-color-1
)

mask: linear-gradient 内容框遮罩 + mask-composite: exclude
opacity: framer-motion 驱动（IDLE:0 → FLOWING:1.0）
--flow-angle: framer-motion 驱动旋转动画
```

### 6.3 GlassPanel（修改）

改动：在现有 `<div>` 外包裹 `<FlowSurface variant="panel">`，新增 `disabled` prop。

```
之前:
  <div className={classes} style={style}>{children}</div>

之后:
  <FlowSurface variant="panel" disabled={disabled}>
    <div className={classes} style={style}>{children}</div>
  </FlowSurface>
```

### 6.4 GlassButton（修改）

交互流程升级：

```
default  → 玻璃底 + 静态
hover    → FlowSurface 进入 FLOWING
           → 边框色带旋转 + 光晕扩大
           → borderRadius 10px → 12px
click    → 从光标位置爆开圆形涟漪
           (motion.div 0→100% scale, opacity 1→0, 400ms)
release  → 回到 hover 或 DISSIPATING
```

### 6.5 GlassInput（修改）

Focus 时边框光流激活，类比按钮的 FLOWING 状态。

### 6.6 ResizeHandle（修改 — 拖拽光轨）

```
close (<20px) → 发光条 + 胶囊图标 + 延伸光晕
dragging      → 发光条 + 鼠标位置光轨
                (motion.div 跟随鼠标, 0.3s 延迟拖尾)
release       → 光轨 1s 消散
```

---

## 7. 布局改动

### 7.1 App.css 布局更新

```
之前:
  .app-root
    Toolbar (36px)
    .app-body
      Sidebar | Main(terminal + transmission) | SendBar
    StatusBar (26px)

之后:
  .app-root
    Toolbar (36px) + BottomBorderGlow   ← 底部流光边框
    .app-body
      FlowSurface(Sidebar)               ← 完整流动表面
        | Main(terminal + transmission)   ← 终端区 disabled
        | FlowSurface(SendBar)           ← 流动表面
    StatusBar (26px) + TopBorderGlow    ← 顶部流光边框
```

### 7.2 ThemeContext 修改

```typescript
// 移除旧主题
export type ThemeId = "prismatic" | "obsidian" | "crystal";

export const THEMES = [
  { id: "prismatic", name: "炫彩", nameEn: "Prismatic" },
  { id: "obsidian",   name: "液态黑曜石", nameEn: "Liquid Obsidian" },
  { id: "crystal",    name: "液态水晶", nameEn: "Liquid Crystal" },
];

// 默认主题: obsidian (暗黑更适合终端日常使用)
```

---

## 8. useFlowInteraction Hook

### 接口

```typescript
interface FlowInteractionState {
  mousePos: { x: number; y: number };  // 相对于面板 0-1
  velocity: number;                     // 光标速度 (px/frame)
  state: "idle" | "aware" | "flowing" | "dissipating";
  frequency: number;                    // 当前湍流频率
  gradientOffset: { x: number; y: number }; // 渐变偏移百分比
  borderOpacity: number;               // 边框流光 0-1
}

function useFlowInteraction(
  ref: RefObject<HTMLElement>,
  options?: {
    variant?: "panel" | "toolbar" | "button" | "handle";
    disabled?: boolean;
  }
): FlowInteractionState;
```

### 内部逻辑

1. `mousemove` 监听 → 计算面板相对位置 + 光标速度
2. `mouseenter` → IDLE/AWARE → AWARE/FLOWING
3. `mouseleave` → FLOWING/AWARE → DISSIPATE
4. `requestAnimationFrame` 中更新频率和渐变偏移
5. 使用 framer-motion `useMotionValue` 确保 GPU 合成

---

## 9. 实施策略

### 阶段 1: 基础令牌 + 主题切换
1. 创建 `tokens-v3.css`
2. 修改 `ThemeContext.tsx` → 新主题 ID
3. 更新 `global.css` 引用
4. 验证主题切换后全局样式正常

### 阶段 2: 核心组件
1. 实现 `useFlowInteraction` hook
2. 实现 `FlowSurface.tsx` + SVG 滤镜
3. 实现 `BorderGlow.tsx`
4. 单元测试 hook 逻辑

### 阶段 3: 现有组件集成
1. 修改 `GlassPanel` → 集成 FlowSurface
2. 修改 `GlassButton` → 流动交互
3. 修改 `GlassInput` → 流动 focus
4. 修改 `GlassPanel.module.css` / `GlassButton.module.css`

### 阶段 4: 布局组件升级
1. 修改 `Toolbar` + 底部流光
2. 修改 `SessionSidebar` + 流动表面
3. 修改 `StatusBar` + 顶部流光
4. 修改 `ResizeHandle` → 拖拽光轨

### 阶段 5: 微调
1. 三个主题的色彩校准
2. 动画性能调优（will-change、contain、GPU 层）
3. 视觉走查 + 边界情况
4. 更新 README 截图
```

---

## 10. 风险与注意事项

- **性能**: SVG feTurbulence 在移动/低配设备上可能丢帧。通过 `will-change: filter` 和仅 FLOWING 状态启用高频率来缓解
- **Tauri 兼容性**: Chromium webview 对 SVG 滤镜的 GPU 加速跨版本一致性好，但需在 Windows/macOS/Linux 三端验证
- **降级策略**: `@supports (backdrop-filter: blur())` 检测，不支持时回退到纯色半透明背景
- **旧主题迁移**: 用户在 localStorage 中 `tauterm-theme` 值为旧主题 ID 时，自动迁移到对应新主题
