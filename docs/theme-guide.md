# TauTerm 主题开发指南

## 核心原则

**永远不要硬编码颜色值。** 所有颜色、模糊、阴影、边框都必须通过 CSS 自定义属性引用。

## 主题令牌速查

### Level 1 — 全局常量（不随主题变化）

这些令牌在 `:root` 中定义，所有组件直接使用：

```css
/* 在组件 CSS 中使用 */
font-family: var(--font-ui);        /* Inter 等无衬线字体 */
font-family: var(--font-mono);      /* JetBrains Mono 等宽字体 */
border-radius: var(--radius-lg);    /* 圆角: xs(3) sm(6) md(10) lg(14) xl(18) 2xl(24) full(9999) */
padding: var(--spacing-md);         /* 间距: xs(4) sm(8) md(12) lg(16) xl(24) 2xl(32) */
transition: all var(--transition-fast); /* 过渡: fast(150ms) normal(300ms) */
transition: all var(--transition-button); /* 按钮过渡: 0.3s cubic-bezier(0.4, 0, 0.2, 1) */
transition: all var(--transition-input);  /* 输入框过渡: 0.3s ease */
z-index: var(--z-panel);            /* 层级: sidebar(10) panel(20) overlay(30) toast(50) */
blur: var(--blur-xs);               /* 4px — 遮罩层背景模糊（全局常量） */
```

### Level 2 — 主题令牌（3 套主题各自定义）

```css
/* ── 背景 ── */
background: var(--bg-base);         /* 页面底色 */
background: var(--bg-secondary);    /* 次级背景 */
background: var(--block-toolbar-bg);   /* Toolbar 区 */
background: var(--block-sidebar-bg);   /* Sidebar 区 */
background: var(--block-terminal-bg);  /* Terminal 区 */
background: var(--block-sendbar-bg);   /* SendBar 区 */
background: var(--block-statusbar-bg); /* StatusBar 区 */

/* ── 文字 ── */
color: var(--text-primary);         /* 主文字 */
color: var(--text-secondary);       /* 辅助文字 */
color: var(--text-muted);           /* 弱化文字 */

/* ── 强调 ── */
color: var(--accent-primary);       /* 主强调色 */
background: var(--accent-gradient); /* 强调渐变（按钮等） */
box-shadow: 0 0 10px var(--accent-glow); /* 发光效果 */

/* ── 玻璃面板 ── */
border: 1px solid var(--glass-border-default);    /* v3 默认边框（推荐） */
border: 1px solid var(--glass-border);            /* v2 兼容别名 */
border-top: 1px solid var(--glass-border-top);    /* 顶部高光 */
border-left: 1px solid var(--glass-border-left);  /* 左侧次高光 */
box-shadow: var(--glass-shadow-outer);            /* 外阴影 */
box-shadow: var(--glass-shadow-inner);            /* 内高光 */
background: var(--glass-fill);                    /* 玻璃填充渐变 v3（推荐） */
background: var(--glass-bg);                      /* v2 兼容别名 */
backdrop-filter: blur(var(--glass-blur)) saturate(var(--glass-blur-saturate)); /* 玻璃模糊 + 饱和度 */

/* ── 玻璃按钮 ── */
background: var(--glass-button-bg);
border: 1px solid var(--glass-button-border);
/* hover */
background: var(--glass-button-hover-bg);
border-color: var(--glass-button-hover-border);

/* ── 玻璃输入框 ── */
background: var(--glass-input-bg);
border: 1px solid var(--glass-input-border);
box-shadow: var(--glass-input-shadow-inner);
/* focus */
border-color: var(--glass-input-focus-border);
box-shadow: ..., var(--glass-input-focus-glow);

/* ── 状态色 ── */
color: var(--color-success);        /* 成功 */
color: var(--color-error);          /* 错误 */
color: var(--color-warning);        /* 警告 */
color: var(--color-info);           /* 信息 */

/* ── 弹窗/下拉 ── */
background: var(--dialog-bg);            /* 弹窗背景 */
background: var(--select-option-bg);     /* select option 背景 */
box-shadow: var(--dialog-shadow);        /* 弹窗阴影 */
/* select 箭头颜色使用 var(--select-arrow) 令牌，三套主题各自定义 */
```

## 组件开发规范

### CSS Module 组件

```css
/* ✅ 正确：全部使用令牌（v3 主令牌优先） */
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

/* ❌ 错误：硬编码颜色 */
.myComponent {
  background: rgba(0, 0, 0, 0.2);   /* 浅色主题下看不出来 */
  border: 1px solid #fff;            /* 浅色主题下看不到 */
  color: #888;                       /* 固定色不随主题变 */
}
```

### 内联 Style 组件

```tsx
// ✅ 正确
<div style={{
  backgroundColor: "var(--bg-base)",
  color: "var(--text-secondary)",
  border: "1px solid var(--glass-border-default)",
}} />

// ❌ 错误
<div style={{
  backgroundColor: "#0a0a1a",    // 硬编码深色
  color: "#888",                  // 不随主题变
  border: "1px solid rgba(0,255,255,0.15)", // v2 teal 遗留色
}} />

// ⚠️ 例外：极少数场景可以硬编码
color: "#fff";                     // 在强调渐变按钮上（总是白色文字）
background: "#4285F4";             // Google 光球颜色（设计特征）
background: "#EA4335";             // Google 光球颜色（设计特征）
background: "#FBBC05";             // Google 光球颜色（设计特征）
background: "#34A853";             // Google 光球颜色（设计特征）
// 所有状态色必须使用 var(--color-*) token，不要硬编码 #34d399 / #eab308
```

### 新增弹窗

```css
.dialog {
  background: var(--dialog-bg);
  border: 1px solid var(--glass-border-default);
  border-radius: var(--radius-xl);
  box-shadow: var(--shadow-glass), var(--dialog-shadow);
  backdrop-filter: blur(var(--glass-blur));
  -webkit-backdrop-filter: blur(var(--glass-blur));
}
```

### 新增 select 下拉

**推荐方式：直接使用全局 `.liquid-glass-input` class + 少量 select 特有样式**

```css
/* CSS Module — 仅保留 select 特有属性 + 组件级尺寸 */
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
// JSX — `.liquid-glass-input` 全局类提供 bg/border/color/shadow/focus/hover/disabled
<select className={`${styles.select} liquid-glass-input`}>
  <option value="a">A</option>
</select>
```

> `.liquid-glass-input` 已提供：`background`、`border`、`box-shadow`、`color`、`outline`、
> `:focus` 发光、`:hover` 边框、`:disabled` 透明度。CSS Module 只需写 select 特有属性和组件级尺寸。

**select 箭头 data URI 令牌**：`--select-arrow` 包含完整的 SVG data URI，各主题通过 fill 颜色适配：

| 主题 | 箭头颜色 |
|------|---------|
| google-glow | `%23686888`（#686888，弱化文字色） |
| obsidian | `%23666666`（#666666） |
| frosted | `%2394a3b8`（#94a3b8，石板灰） |

### 状态着色背景（color-mix 模式）

使用 `color-mix(in srgb, var(--color-*) N%, transparent)` 创建主题自适应的状态着色背景。这是错误提示框、警告横幅、成功标记和信号徽章的规范写法：

```css
/* ✅ 正确 — 三套主题自适应 */
.errorBanner {
  background: color-mix(in srgb, var(--color-error) 10%, transparent);
  border: 1px solid color-mix(in srgb, var(--color-error) 30%, transparent);
  color: var(--color-error);
}
.warningBanner {
  background: color-mix(in srgb, var(--color-warning) 6%, transparent);
  border-left: 3px solid var(--color-warning);
}
.successBadge {
  background: color-mix(in srgb, var(--color-success) 12%, transparent);
}
.dangerHover:hover {
  background: color-mix(in srgb, var(--color-error) 12%, transparent);
  color: var(--color-error);
}
.signalActive {
  color: var(--accent-primary);
  background: color-mix(in srgb, var(--accent-primary) 10%, transparent);
}

/* ❌ 错误 — 深色背景值在浅色主题下失效 */
.errorBanner {
  background: rgba(255, 71, 87, 0.1);   /* 仅 google-glow 有效 */
}
```

### 新增切换开关

规范的自定义液态玻璃切换开关模式（替换原生 checkbox）：

```css
/* 隐藏原生 checkbox */
.toggleCheck {
  position: absolute;
  opacity: 0;
  width: 0;
  height: 0;
}

/* 自定义轨道 */
.toggleTrack {
  position: relative;
  width: 30px;
  height: 17px;
  background: var(--glass-input-bg);
  border: 1px solid var(--glass-input-border);
  border-radius: var(--radius-full);
  box-shadow: var(--glass-input-shadow-inner);
  transition: all 0.3s cubic-bezier(0.4, 0, 0.2, 1);
  flex-shrink: 0;
}

/* 滑块（::after 伪元素） */
.toggleTrack::after {
  content: "";
  position: absolute;
  top: 2px;
  left: 2px;
  width: 11px;
  height: 11px;
  border-radius: 50%;
  background: var(--text-muted);
  transition: all 0.3s cubic-bezier(0.4, 0, 0.2, 1);
  box-shadow: var(--shadow-sm);
}

/* 选中态：轨道填充强调渐变 + 发光 */
.toggleCheck:checked + .toggleTrack {
  background: var(--accent-gradient);
  border-color: transparent;
  box-shadow: 0 0 10px var(--accent-glow);
}

/* 选中态：滑块变色并右移 */
.toggleCheck:checked + .toggleTrack::after {
  left: 15px;
  background: var(--text-primary);
}

/* 禁用态 */
.toggleCheck:disabled + .toggleTrack {
  opacity: 0.35;
  cursor: not-allowed;
}
```

```tsx
// JSX 用法：
<label className={styles.repeatLabel}>
  <input
    type="checkbox"
    className={styles.toggleCheck}
    checked={enabled}
    onChange={(e) => setEnabled(e.target.checked)}
    disabled={!isConnected}
  />
  <div className={styles.toggleTrack} />
  <span className={styles.toggleText}>⟳</span>
</label>
```


## 全局 CSS 工具类

以下全局 class 由 `global.css` 提供，可在任何组件中直接使用：

| Class | 用途 |
|-------|------|
| `.liquid-glass` | 完整液态玻璃面板效果（含 SVG 噪点纹理 + 不对称高光边框 + 多层阴影） |
| `.liquid-glass-button` | 液态玻璃按钮（半透明底 + 悬浮上浮 + 阴影增强） |
| `.liquid-glass-input` | 液态玻璃输入框（暗色内凹底 + focus 蓝色辉光） |
| `.liquid-primary-button` | 炫彩主动作按钮（全息渐变 + `gradient-shift` 动画 + 玻璃模糊） |
| `.glow-orb` | 光球（用于 GoogleGlowBackground 中的 4 个流动光球） |

```css
/* 典型用法：组合 CSS Module class + 全局 tool class */
<button className={`${styles.myBtn} liquid-primary-button`}>Send</button>
<div className={`${styles.myPanel} liquid-glass`}>Content</div>
```

## 检查清单

新组件合入前自查：

- [ ] 所有 `color` / `background` / `border-color` / `box-shadow` 使用 `var(--xxx)` 令牌
- [ ] 没有 `#xxx` 硬编码（除少数例外场景）
- [ ] 没有 `rgba(0,0,0,x)` 或 `rgba(255,255,255,x)` 硬编码
- [ ] `border-radius` > 4px（使用 `--radius-md` 或更大，推荐 10px+）
- [ ] 弹窗/浮层使用了 `var(--dialog-bg)` 背景 + `backdrop-filter: blur()`
- [ ] select option 使用了 `var(--select-option-bg)`
- [ ] 切换 3 套主题都能正常显示
- [ ] 状态着色背景使用 `color-mix(in srgb, var(--color-*) N%, transparent)` — 禁止硬编码 rgba
- [ ] `z-index` 使用 `var(--z-*)` 令牌 — 禁止裸数字
- [ ] `backdrop-filter` 模糊值使用 `var(--blur-*)` 或 `var(--glass-blur)` 令牌
- [ ] 遮罩/蒙版背景使用 `var(--overlay-bg)` — 禁止硬编码黑色
- [ ] 所有 `<select>` 和 `<input>` 元素使用全局 `liquid-glass-input` class 获取基础视觉，CSS Module 仅保留组件级差异化属性（尺寸、箭头、option）
- [ ] 自定义 SVG data URI（如 select 箭头）的硬编码填充色需在注释中注明

> **注意**：`tokens.css` 中定义的 `--glass-noise-frequency` 目前未被 `global.css` 的噪点 SVG 实际使用（三套主题的 `baseFrequency` 差异微乎其微，统一为 0.8）。新增主题时不需定义此令牌。
>
> **禁用态透明度规范**：按钮禁用态统一使用 `opacity: 0.5`，输入框/选择框禁用态统一使用 `opacity: 0.4`。

## 三套主题色板速览

### 核心令牌（v3 主令牌）

| 令牌 | google-glow | obsidian | frosted |
|------|------------|----------|---------|
| `--bg-base` | `#080808` | `#030303` | `#f8fafc` |
| `--text-primary` | `#e0e0ff` | `#e6e6e6` | `#1e293b` |
| `--glass-fill` | `linear-gradient(135deg, rgba(255,255,255,0.08) 0%, rgba(255,255,255,0.02) 100%)` | `linear-gradient(135deg, rgba(20,20,25,0.6) 0%, rgba(5,5,10,0.4) 100%)` | `linear-gradient(135deg, rgba(255,255,255,0.7) 0%, rgba(255,255,255,0.4) 100%)` |
| `--glass-border-default` | `rgba(255,255,255,0.15)` | `rgba(255,255,255,0.05)` | `rgba(148,163,184,0.35)` |
| `--glass-blur` | `25px` | `30px` | `35px` |
| `--glass-noise-opacity` | `0.04` | `0.05` | `0.03` |
| `--glass-blur-saturate` | `100%` | `120%` | `150%` |
| `--accent-primary` | `#4285F4` | `#4285F4` | `#3b82f6` |
| `--dialog-bg` | `rgba(15,15,30,0.95)` | `rgba(8,8,12,0.97)` | `rgba(255,255,255,0.92)` |
| `--select-option-bg` | `#12122a` | `#0a0a12` | `#ffffff` |
| `--overlay-bg` | `rgba(0,0,0,0.5)` | `rgba(0,0,0,0.6)` | `rgba(0,0,0,0.2)` |
| `--bg-orb-opacity` | `0.65` | `0.45` | `0.35` |
| `--bg-orb-blur` | `120px` | `140px` | `140px` |
| `--bg-orb-blend` | `screen` | `screen` | `multiply` |

### 辅助令牌（跨组件共享的状态与层级令牌，所有 3 套主题各自定义）

这些令牌不是临时兼容层，而是被大量组件引用的稳定 API。它们与 v3 主令牌共同构成完整的设计系统。

| 辅助令牌 | 说明 |
|---------|------|
| `--glass-bg` | 玻璃纯色背景（v3 `--glass-fill` 为渐变，两者语义不同，并存） |
| `--glass-bg-hover` / `--glass-bg-active` | 悬浮/激活态背景 |
| `--glass-border` | 默认玻璃边框 |
| `--glass-border-hover` / `--glass-border-focus` | 悬浮/聚焦态边框 |
| `--blur-light` / `--blur-medium` / `--blur-heavy` | 分级模糊值（8px / 16-20px / 24-35px） |
| `--shadow-glass` / `--shadow-elevated` / `--shadow-sm` | 分级阴影系统 |
| `--bg-secondary` | 次级背景色 |
