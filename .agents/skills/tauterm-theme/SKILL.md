---
name: tauterm-theme
description: "Single source of truth for TauTerm Liquid Glass UI, shared Google Ambient Fields, glass physics, material layering, control contrast, visual performance tiers, and rendering-performance rules. Use for any React/CSS UI creation or modification, theme work, visual review/fixes, animation, controls, glass/backdrop effects, or appearance/performance settings."
license: MIT
metadata:
  author: tauterm
  version: "7.1"
---

# TauTerm Liquid Glass v7.1 — 唯一主题规范源

> **SSOT**：本文件是 TauTerm 视觉材质、Google Ambient、主题 tint、控件状态与渲染性能规则的唯一规范源。  
> `tauterm-theme-review` 只描述审查流程，不得维护第二份视觉规则。

## 核心模型

TauTerm 使用一套跨主题共享的 **Liquid Glass Physics**：

1. Google Ambient 提供颜色。
2. Glass 本身尽量中性，通过 transparency / specular / edge / shadow 表达材质。
3. Theme 主要改变 tint：Smoke / Obsidian / Frost。
4. Performance 改变动态和 sampling 成本，不改变主题身份。
5. Content 与 Control 都不得依赖大面积实时 blur。

目标不是“毛玻璃”，而是：

- 背景颜色可感知，但不干扰文字。
- 玻璃靠薄边缘和光学层次成立，而不是靠 neon glow。
- 老设备上没有大面积实时 blur / blend / morph。
- 控件无论处在最亮黄色 Ambient、最暗区域还是 Frosted 白底上，都必须保持清楚轮廓。

---

## 1. 五层 Surface 语义

### A. Chrome — `.liquid-glass`

用于 Toolbar、左右 Sidebar 外壳、StatusBar、Dialog / Settings、固定小面积 chrome。

- 允许轻量 `backdrop-filter`。
- blur 只由 `--glass-blur-chrome` 和 performance 控制。
- 使用中性 tint + specular + edge + soft shadow。
- 禁止用于 Terminal、数据画布、SendBar 主体。

### B. Content — `.liquid-glass-content`

用于 Terminal pane、日志/数据展示、Network/TFTP/iperf 主内容和空 Pane。

- **禁止 backdrop-filter**。
- 背景为 `--content-surface`。
- active 使用 `.liquid-glass-content-active`，必须比 inactive 更透。
- 目标是看到 Ambient 的低频颜色，不让背景决定文字可读性。

### C. Control Surface — `.liquid-control-surface`

用于高密度交互工作台：

- SendBar 主体与 TargetBar
- 大面积表单工作区
- 参数编辑器
- 复杂输入/执行工具

规则：

- **禁止 backdrop-filter**。
- 比 Content Surface 更稳定、更实。
- 使用 `--control-surface` / `--control-surface-border` / `--control-surface-shadow`。
- Control Surface 必须局部重绑标准 button/input token（`--control-button-*` / `--control-input-*`），使内部所有共享控件自动获得更强轮廓。
- Disabled input 使用独立 `--control-disabled-input-bg`，不能与 Control Surface 本体融成一块。
- Ambient 可以轻微影响整体 tint，但不能让 button/input/select/toggle/number stepper 的轮廓随背景颜色消失。

### D. Accent — `.liquid-glass-accent`

用于 search trigger、active pill、segmented control、小型 selected/hover 区域。

- 默认不常驻 backdrop blur。
- 用 tint、specular、短位移表达交互，不做常驻 neon glow。

### E. Float — `.liquid-glass-float`

用于 ContextMenu、Toast、Popover、Search dropdown、portal 浮层。

- 可使用 `--glass-blur-float`。
- Compatibility 自动关闭 backdrop sampling。

### Nested Card

内嵌区域使用 `.liquid-glass-card` / `.liquid-glass-mini-card` / `.liquid-glass-status-card`，禁止在 glass 内继续套大面积 `.liquid-glass`。

---

## 2. Shared Google Ambient Field

Google Ambient 是 TauTerm 的品牌背景，三主题完全共享。**Google/Gemini 品牌色只有这一套 canonical palette**：

- `--google-blue: #4285F4`
- `--google-red: #EA4335`
- `--google-yellow: #FBBC05`
- `--google-green: #34A853`
- `--google-brand-gradient` 只能由以上四个 token 组合

实现规则：

- 代码里的品牌色十六进制值只允许出现在 `src/styles/tokens.css` 的 canonical palette 定义块。
- Ambient、Google Glow selected/primary、主题预览、后续任何 Gemini/Google 光效都必须引用 `--google-*` / `--google-brand-gradient`。
- 禁止另外定义“接近 Google”的紫、粉、蓝、黄作为品牌色。
- 渐变插值自然产生的紫/橙过渡是允许的，但它们不是独立 token，也不能被组件硬编码。

### 架构

使用 **2 个 oversized Field**，不使用 4 个可见圆形 Orb：

- Field A：Blue + Yellow
- Field B：Red + Green
- Field 约 140vw × 136vh，并超出 viewport。
- 每个 radial-gradient 使用长 falloff，在 Field 边界前已经近乎透明。
- 颜色必须跨越 pane divider，不允许长期表现成“左上蓝 / 右上红 / 左下黄 / 右下绿”的四象限。

### 动画

- 只允许 `transform`。
- 使用闭合多段轨迹 + `linear`，避免 ease-in-out 长时间像静止。
- 两个 Field 必须使用不同 phase、方向和轻微 rotation，形成可感知的相对剪切运动。
- Quality：约 18–23s，较大路径。
- Balanced：约 27–33s，较缓慢路径。
- 正常观察 3–5 秒应能确认色场正在移动；如果只有截图切换时才感知运动，视为失败。
- Compat：只保留 1 个静态 Field。
- 不使用 element `filter: blur`、`mix-blend-mode`、morph、常驻 `will-change`。

### Motion 状态

`ThemeContext` 写入：

- `full`
- `reduced`
- `paused`

规则：

- **窗口失焦但仍可见时不得暂停 Ambient。**
- 只有 `document.hidden` 时进入 `paused`。
- screenshot 工具、Alt-Tab 焦点变化不得造成 Ambient 跳变/冻结。
- `prefers-reduced-motion` 停止装饰 Ambient。
- 只能暂停 `.ambient-field`；**禁止用 `html[data-motion="paused"] *` 冻结全站 animation**，避免破坏 loading/progress/status 语义动画。

---

## 3. Theme = Tint，不是另一套 Physics

三个主题使用同一 Ambient、圆角体系、surface 分类、交互规则和 performance 机制。

### Google Glow — Smoke

第一印象：**中性深烟晶 + 彩色环境光**

- Base 接近 neutral charcoal，不允许全屏 Navy Blue。
- Glass 不能主动染成蓝色。
- Google 四色 Ambient 是主要色彩来源。
- Content 三主题中偏透。
- Google Glow 必须保留独有的高价值品牌渐变，但颜色源只能是 canonical Google/Gemini 四色：Blue → Red → Yellow → Green。
- `.liquid-theme-selected` 与 `.liquid-primary-button` 可使用 `--google-brand-gradient`，只限 selected tab / active mode / 主 CTA 等少量高价值状态。
- 中间出现的 Purple / Orange 只能来自四色渐变的自然插值，不允许独立定义为 Google/Gemini 品牌色。
- 普通 button/chrome 不得全息化；Accent 主色仍引用 `--google-blue`。

### Obsidian — Black

第一印象：**更深、更实的黑曜石 tint**

- 相同 Ambient。
- Glass Physics 与 Google Glow 相同。
- 通过更黑 base、更实 content/control tint、更低亮度 edge 形成身份。
- Obsidian 的 selected/primary surface 使用石墨蓝灰膜，不继承 Google Glow 的彩色 holofoil。
- 不另外换成蓝紫 Ambient。

### Frosted — White

第一印象：**银白 / 冰霜 tint**

- 相同 Ambient RGB / geometry / motion / opacity。
- Google 色通过白色材质自然表现为较浅颜色，不另建 pastel palette。
- 需要同时拥有亮顶部 specular 和微弱的暗边缘，不能退化成纯白平面。

---

## 4. Performance 正交原则

### `quality` — 效果优先

- 2 个完整 Ambient Field。
- 更短 duration、更大路径。
- chrome / float 可使用 16–18px 级 sampling。
- Content / Control blur 始终为 0。

### `balanced` — 默认

- 2 个完整 Ambient Field。
- 更慢、更克制的 transform 路径。
- chrome / float 约 8–10px sampling。
- Content / Control blur 始终为 0。

### `compat`

- 1 个静态 Ambient Field。
- chrome / float backdrop blur = 0。
- Material、control contrast、主题 tint 仍完整保留。

Theme 与 Performance 不得互相编码：切性能档不能改变主题 tint 或 Google RGB。

---

## 5. Control Contrast

透明 UI 中，控件不能建立在“背景永远纯黑”的假设上。

### 禁止整个 disabled 控件降 opacity

禁止：

```css
button:disabled { opacity: .4; }
input:disabled { opacity: .4; }
```

因为这会同时淡掉 background / border / text / icon，让动态 Ambient 直接穿透控件。

统一使用：

- `--control-disabled-bg`
- `--control-disabled-input-bg`
- `--control-disabled-border`
- `--control-disabled-text`
- `--control-disabled-icon`

全局 `.liquid-glass-button`、`.liquid-primary-button`、`.liquid-glass-input`、`.liquid-glass-toggle` 负责 disabled material。

组件 CSS Module 不得再次降低整个 disabled 控件 opacity。

允许降低**内部非结构性子元素**（例如数字框微调箭头、loading content），但不能让 control silhouette 消失。

### Disabled 验收

同一个 disabled control 必须在以下位置仍可辨认：

1. Ambient 最亮黄色区域
2. Ambient 最暗区域
3. Frosted 白色区域

Disabled 表示“不可操作”，不是“不可见”。

---

## 6. Input / Button 光学层次

### Input

输入控件应像稳定的轻微凹槽：

- 背景比所在 Control Surface 稍深/稍实。
- border 必须能跨不同 Ambient 亮度保持轮廓。
- 使用轻 inset top/bottom edge。
- focus 允许 1–2px accent ring，不允许 20px neon glow。

### Button

- 默认状态保持清楚 silhouette。
- hover 只做轻 tint / edge / 最多 1px lift。
- 普通 active 用 accent tint。
- 主题身份型 selected 状态使用 `.liquid-theme-selected`。
- Google Glow 的主题 selected 使用静态 `--google-brand-gradient`，并只在 hover 时 transition background-position；禁止 idle 无限渐变动画。
- disabled 保持 shape 与 edge，降低 text/accent 强度。

### Toggle / Checkbox / Number

- 自绘 track 必须与全局 disabled tokens 一致，不得使用 `opacity: .35` 整体淡出。
- number stepper 箭头不能依赖 data-URI SVG 的 `currentColor` 继承；WebView 中应使用主题 token 提供明确 SVG 颜色。
- disabled stepper 可以降低内部箭头 opacity，但整个 input silhouette 必须保持完整。

---

## 7. Backdrop-filter 红线

`backdrop-filter` / `-webkit-backdrop-filter` 只能由 `src/styles/global.css` 的全局 Chrome/Float 材质实现。

CSS Module 出现 backdrop-filter 默认视为 HIGH/CRITICAL。

尤其禁止：

- Terminal/xterm
- SplitView 大面积 Pane
- SendBar / TargetBar
- File/data/log 主体
- 图表
- 大面积表单/编辑区
- glass 内再嵌 glass

---

## 8. Terminal / SplitView

- xterm `background: "transparent"` 时必须 `allowTransparency: true`。
- xterm 透明目标是 Content Surface，不是 raw window。
- Google Glow 的 xterm cursor / ANSI Blue/Red/Yellow/Green 必须在运行时读取 `--google-*` canonical token；TS 中不得复制 Google 品牌十六进制值。
- ANSI Purple/Cyan/bright variants 可以从 canonical 四色派生混色，但不能被定义成新的 Google/Gemini 品牌 token。
- inactive terminal 保留实例/scrollback，淡出后 `visibility:hidden`。
- active pane 比 inactive 更透。
- 不使用未经验证的 `content-visibility`。
- resize gesture 可临时关闭 chrome backdrop sampling。

---

## 9. CSS 性能规则

禁止：

- `transition: all`
- 大面积 `filter: blur`
- `mix-blend-mode`
- 持续 background gradient animation
- 常驻 `will-change`
- 无条件 `translateZ(0)` / `backface-visibility:hidden`

只 transition 实际变化的属性。

拖拽状态、loading 内容、非结构性图标可以使用局部 opacity；不能把 opacity 当作 glass material 或 disabled material。

---

## 10. Modal

统一使用 `.glass-overlay`。

- Appearance 没有特殊 Preview Overlay。
- overlay 不应彻底抹掉主题身份。
- Dialog 是 Chrome，可使用性能档允许的小面积 backdrop sampling。

---

## 11. Sidebar

Sidebar 外壳是 Chrome；session item 默认扁平：

- default：透明 border / 无常驻卡片阴影
- hover：轻 tint + edge
- active：明确 tint + 轻 rim

不要让几十个会话项成为几十层常驻玻璃卡。

---

## 12. GlassPanel API

`GlassPanel.surface`：

- `chrome`
- `content`
- `control`
- `accent`

高密度交互区必须使用 `control`，大面积内容画布使用 `content`。

---

## 13. 图标 / Emoji

除 `src/components/FileManager/entryIcon.ts` 的文件类型 emoji 外，UI 不用 emoji 作为控件图标。使用 `Icon` + `src/assets/icons/`。

---

## 14. 提交前审计

```bash
# 大面积 / 组件私建 backdrop
rg 'backdrop-filter' src/components src/renderers --glob '*.module.css'

# 禁止 transition all
rg 'transition:\s*all' src --glob '*.css'

# 禁止高成本视觉路径
rg 'filter:\s*blur|mix-blend-mode|will-change' src --glob '*.css' --glob '*.tsx'

# disabled 整体 opacity
rg -U ':disabled[^\{]*\{[^\}]*opacity\s*:\s*0\.' src --glob '*.css'

# surface 分配
rg 'liquid-glass-content|liquid-control-surface|liquid-glass-float|liquid-glass' src --glob '*.tsx'

# Ambient 旧 Orb / 全局动画暂停不可回归
rg 'glow-orb|data-motion="paused".*\*' src --glob '*.css' --glob '*.tsx'

# Canonical Google/Gemini palette：品牌十六进制只能出现在 tokens.css canonical block
rg '#4285F4|#EA4335|#FBBC05|#34A853' src --glob '*.css' --glob '*.tsx' --glob '*.ts'

npm run build
```

## 15. 视觉验证矩阵

必须覆盖：

- 3 themes × 3 performance modes
- single pane / 4 pane
- Ambient 运行至少 10 秒
- app focus / screenshot / Alt-Tab 后 Ambient 连续
- minimized/hidden 后 Ambient 暂停
- SendBar connected / disconnected
- RightSidebar transfer enabled / disabled
- Settings / ConnectDialog enabled / disabled controls
- Ambient 黄、蓝、绿、红区域下的控件对比度
- Frosted 上的控件边缘
- 多后台 xterm + 高速输出
- resize / split drag

验收原则：

- **Ambient 是连续色场，不是四个圆片**
- **Google Glow 是 Smoke，不是 Navy Blue**
- **Glass 负责光学层次，Ambient 负责颜色**
- **Disabled 可辨认但不活跃**
- **Content 透明，Control 稳定**
- **Quality 更活，Balanced 更缓，Compat 真静态**
- **Google Glow 的 selected 状态一眼可辨，Obsidian 不得看起来只是同一套蓝按钮**
- **SendBar 在 disconnected 状态仍能看清 textarea/select/toggle/number/history/send 的完整轮廓**

## 实现源文件

- `src/styles/tokens.css`
- `src/styles/global.css`
- `src/context/ThemeContext.tsx`
- `src/components/Layout/GoogleGlowBackground.tsx`
- `src/components/Terminal/Terminal.tsx`
- `src/components/Terminal/TerminalView.tsx`
- `src/components/Layout/SplitView.tsx`
- `src/components/SendBar/SendBar.tsx`
