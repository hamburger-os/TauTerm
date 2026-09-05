---
name: tauterm-theme
description: "Single source of truth for TauTerm Liquid Glass UI, Gemini/Google ambient spectrum, structural panels, control states, split/session presentation, motion, and rendering-performance rules. Use for any React/CSS visual modification, theme work, animation, panel/control styling, or UI rendering-performance change."
license: MIT
metadata:
  author: tauterm
  version: "8.0"
---

# TauTerm Liquid Glass v8.0 — 唯一主题规范源

> **SSOT**：TauTerm 的主题、材质、Gemini 色谱、视觉动画、Structural Panel、SendBar、SplitView 视觉状态与渲染性能规则只在本文件维护。  
> `docs/` 不复制主题规则；`tauterm-theme-review` 只维护审查流程。

## 1. 设计模型

TauTerm 的视觉由四件事组成，彼此正交：

1. **Gemini Ambient**：负责低频颜色与运动。
2. **Liquid Glass Material**：负责透明度、边缘、高光和阴影。
3. **Theme Tint**：Google Glow / Obsidian / Frosted 只改变材质 tint，不改变色谱与物理规则。
4. **Performance Mode**：Quality / Balanced / Compat 只改变采样与动画成本，不改变主题身份。

基本原则：

- 颜色来自 Ambient；玻璃本体保持中性。
- 大面积工作区不依赖实时 backdrop blur。
- 同级结构必须复用同一个 surface class，不允许“看起来差不多”的组件私有实现。
- 视觉装饰不能引入持续 ResizeObserver、持续 layout 读取或高成本 filter。
- CSS Module 不维护另一套主题参数；颜色、材质、motion 关键值集中在 `src/styles/tokens.css` / `src/styles/global.css`，规范只由本 skill 定义。

---

## 2. Canonical Gemini Spectrum

唯一品牌色：

- `--google-red: #FE3734`
- `--google-yellow: #F4BA00`
- `--google-green: #02BE66`
- `--google-blue: #0B8AFF`

对应 RGB token 只用于 alpha；组件不得复制品牌 hex。

### 空间顺序

Gemini mark 的二维空间位置必须固定为：

- **左上：Red**
- **左下：Yellow**
- **右下：Green**
- **右上：Blue**

若按左侧自上而下，再沿底部到右侧自下而上读取，就是 **Red → Yellow → Green → Blue**。

`--google-brand-gradient` / `--google-brand-gradient-soft` 必须一次同时呈现四个区域；不能用很长的线性渐变配合 `background-position`，导致 idle 只看到“黄→蓝”、hover 又变成“蓝→红”。

Purple / Orange 只能是四色插值结果，不定义为额外品牌 token。

---

## 3. Gemini Ambient Flow

### 架构

使用两个 oversized、静态 raster gradient Field：

- **Field A：Red（左上） + Green（右下）**
- **Field B：Blue（右上） + Yellow（左下）**

推荐尺寸约 **128vw × 126vh**，边缘在 viewport 外衰减为透明。

这样两个对角 Field 反向运动时会形成交叉剪切和颜色交换，但整体仍能辨认四个正确象限；禁止把颜色永久锁成四块硬分区，也禁止把四个圆形 Orb 直接放在四角。

### 动画

只允许 transform：

- translate + 轻 rotation + 轻 scale；
- 两个 Field 的 phase、方向和周期不同；
- linear 闭合轨迹，避免 ease-in-out 长时间像静止；
- Balanced 约 **20s / 24s**；
- Quality 约 **14s / 17s**，路径稍大；
- 正常观察 **3–5 秒必须明显感知正在流动**。

禁止：

- `filter: blur()`
- `mix-blend-mode`
- 持续 background-position / gradient stop 动画
- morph
- 常驻 `will-change`

### Motion / Performance

- 窗口失焦但仍可见：Ambient 继续。
- 只有 `document.hidden` 才 paused。
- `prefers-reduced-motion`：停止装饰动画。
- Compat：**保留完整四色的两个静态 Field**，不能为了省一个 layer 丢掉半套品牌色。
- 不能用全局 `*` 暂停动画，避免冻结 loading/status 等语义动画。

---

## 4. Surface 体系

### A. Small Chrome — `.liquid-glass`

用于 Toolbar、Dialog、Settings、小面积固定 chrome。

- 可按 performance 使用小面积 backdrop sampling。
- 不用于 Sidebar、SendBar、Terminal、数据画布。

### B. Structural Panel — `.liquid-glass-panel`

用于：

- 左 Sidebar 外壳
- 右 Sidebar 外壳
- SendBar 主壳
- TargetBar

这是四者的**唯一结构材质**。

规则：

- 四者必须使用同一个全局 class 和同一组 `--panel-*` token。
- **禁止 backdrop-filter**。
- 比 Small Chrome 更透明，通过 specular top/left edge + 很轻的 outer/inset shadow 建立液态玻璃。
- 不允许左栏两层 glass、右栏一层 glass 这种嵌套差异。
- 外壳只出现一层 Structural Panel；内部列表/控件按自身语义绘制。
- 不使用纯装饰性的 ResizeObserver 来维持阴影、渐隐或材质状态。

### C. Content — `.liquid-glass-content`

Terminal、Network/TFTP/iperf 数据区、空 Pane 等大面积内容区。

- backdrop blur = 0。
- 多分屏时 active 可比 inactive 稍透。
- **单 Pane 时不显示 selected/active 视觉差异**。

### D. Control Surface — `.liquid-control-surface`

用于确实需要更稳定底色的高密度参数编辑器/表单内部区域。

- backdrop blur = 0。
- 不再作为 SendBar/TargetBar 外壳。
- 必须保证 input/select/toggle/number 在最亮 Ambient 上也有清楚 silhouette。

### E. Accent / Float

- `.liquid-glass-accent`：小型 active/selected 区。
- `.liquid-glass-float`：popover/context menu/toast；可按 performance 小面积 backdrop sampling。

---

## 5. Theme = Tint

三主题共用：

- 同一 Gemini RGB
- 同一 Ambient geometry / motion
- 同一 Structural Panel 语义
- 同一圆角和交互规则

### Google Glow

中性 Smoke + 彩色环境光。禁止把整个应用染成 Navy Blue。

### Obsidian

更深、更实的黑曜石 tint；品牌 hue 不换成蓝紫或蓝灰。

### Frosted

银白 / 冰霜 tint；仍使用同一 Gemini 色谱，不另造 pastel palette。

---

## 6. Gemini Prism Button / Selected

`.liquid-primary-button` 与高价值 `.liquid-theme-selected` 可以使用 canonical 四色 Prism。

### Idle

- 一次同时显示四种颜色。
- 空间关系遵守：左上红 / 左下黄 / 右下绿 / 右上蓝。
- 静态，无无限渐变动画。

### Hover

**绝不改变颜色顺序或切换 gradient window。**

只允许：

- `translateY(-1px)`
- 约 `scale(1.025–1.04)`
- 稍强 border / specular / shadow

这是默认美术策略：让按钮像一块被抬起的彩色玻璃，而不是鼠标移入后“换了一套配色”。

### Active

轻微压下，例如 `scale(.98)`。

普通按钮不全息化。

---

## 7. SendBar

四个模式必须共享一个清楚、连续的工作台轮廓：

- Basic
- Command
- Auto Reply
- Script

### 布局

- 模式切换器使用**横向顶部模式条**，不要再用左侧竖排四个孤立按钮。
- 当前模式按钮使用 `.liquid-theme-selected`，其余为普通 glass button。
- 模式条右侧可显示当前模式标题，强化当前上下文。
- 内容区四模式可保持 mounted，只切 CSS visibility/display，避免切换时丢状态。
- TargetBar 与主体之间允许 4px 级结构间距，但材质必须同为 `.liquid-glass-panel`。
- SendBar 的最小高度由“顶部模式条 + 内容最低高度”推导，不再与 4 个竖排按钮总高度绑定。

### 视觉验收

四种模式截图中都必须能一眼看到：

1. 完整 panel silhouette；
2. 顶部模式条；
3. 当前模式；
4. 内容区域；
5. 底部执行/状态控件。

不能出现只有一个炫彩按钮和几个控件“漂浮”在 Ambient 上的效果。

---

## 8. Sidebar

左右 Sidebar 的外壳必须完全同构：

- App 层各只有一层 `.liquid-glass-panel`；
- SessionSidebar / RightSidebar 内部只负责布局，不再叠加 `.liquid-glass`；
- 相同 radius / border / specular / shadow / transparency；
- session item 默认扁平，hover/active 才出现轻 tint；
- 不为滚动提示引入额外材质层或持续 observer。

---

## 9. SplitView / Session Empty State

### 单 Pane

单 Pane 没有比较对象，因此：

- 不显示 selected top line；
- 不显示 selected frame；
- Content 不区分 active/inactive tint。

### 多 Pane

只有 `paneCount > 1` 时才显示 selected chrome。

### Disconnected

“连接后显示会话内容”的空状态必须由 SplitView 的共享组件绘制：

- 同一 connection icon；
- 同一圆形 icon frame；
- 同一字号；
- 同一间距与颜色。

Network Debug 断开时不得让 renderer 自己维护另一套空状态。其它会话需要相同语义时也优先复用这个共享组件。

---

## 10. Control Contrast

禁止整体 disabled opacity：

```css
button:disabled { opacity: .4; }
input:disabled { opacity: .4; }
```

统一依赖：

- `--control-disabled-bg`
- `--control-disabled-input-bg`
- `--control-disabled-border`
- `--control-disabled-text`
- `--control-disabled-icon`

Input 应像稳定的浅凹槽；focus 只允许克制的 1–2px ring，不使用大范围 neon glow。

---

## 11. Terminal / Runtime

- xterm transparent 时必须 `allowTransparency: true`。
- xterm 背景透明到 Content Surface，不透明到 raw window。
- Google Glow ANSI 基色运行时读取 canonical `--google-*` token。
- inactive terminal 保留实例/scrollback，可隐藏 paint，不销毁 runtime。
- resize gesture 可临时关闭允许的 chrome backdrop sampling。

---

## 12. Backdrop / CSS 性能红线

`backdrop-filter` / `-webkit-backdrop-filter` 只能由 `src/styles/global.css` 的 Small Chrome / Float 实现。

禁止：

- CSS Module 私建 backdrop
- Terminal / SplitView / Sidebar / SendBar / TargetBar 大面积 backdrop
- `transition: all`
- 大面积 `filter: blur`
- `mix-blend-mode`
- 持续 gradient/background animation
- 常驻 `will-change`
- 无条件 `translateZ(0)`
- 纯装饰目的的持续 DOM size observer / layout polling

---

## 13. Performance Modes

### Quality

- 2 个动态 Ambient Field
- 更短周期 / 稍大 transform path
- Small Chrome / Float 可用较高 blur sampling

### Balanced（默认）

- 2 个动态 Ambient Field
- 20s / 24s 级低成本 transform
- Small Chrome / Float 约 8–10px sampling

### Compat

- 2 个静态 Ambient Field，完整四色
- backdrop blur = 0
- Panel/Content/Control 材质仍完整存在

Performance 不得改变主题 palette 或布局身份。

---

## 14. 提交前审计

```bash
rg 'backdrop-filter' src/components src/renderers --glob '*.module.css'
rg 'transition:\s*all' src --glob '*.css'
rg 'filter:\s*blur|mix-blend-mode|will-change' src --glob '*.css' --glob '*.tsx'
rg -U ':disabled[^\{]*\{[^\}]*opacity\s*:\s*0\.' src --glob '*.css'
rg 'liquid-glass-panel|liquid-glass-content|liquid-control-surface|liquid-glass-float|liquid-glass' src --glob '*.tsx'
rg 'glow-orb|data-motion="paused".*\*' src --glob '*.css' --glob '*.tsx'
rg '#FE3734|#F4BA00|#02BE66|#0B8AFF|#4285F4|#EA4335|#FBBC05|#34A853' src --glob '*.css' --glob '*.tsx' --glob '*.ts'
npm run build
```

---

## 15. 视觉验证矩阵

至少验证：

- 3 themes × 3 performance modes
- single pane / 2 pane / 4 pane
- Ambient 连续运行 ≥10s
- focus / Alt-Tab / screenshot 后 motion 连续
- hidden/minimized pause
- SendBar 四模式
- TCP/UDP Network Debug connected / disconnected
- 左右 Sidebar 同时可见
- TargetBar visible / hidden
- disabled controls 位于 Red / Yellow / Green / Blue 最亮区域
- Frosted 控件边缘
- 多后台 xterm + 高速输出
- resize / split drag

验收句：

- **Red 左上，Yellow 左下，Green 右下，Blue 右上**
- **3–5 秒能看出 Ambient 在流动**
- **按钮 hover 抬起，不换色**
- **左右栏与 SendBar 是同一种透明 Structural Glass**
- **SendBar 四模式有完整工作台轮廓**
- **Network Debug 断开态与其它 Pane 共用空状态**
- **单 Pane 没有多余蓝色选中线**
- **Compat 保留完整四色且不做动态合成**

---

## 实现源文件

- `src/styles/tokens.css`
- `src/styles/global.css`
- `src/context/ThemeContext.tsx`
- `src/App.tsx`
- `src/components/Layout/GoogleGlowBackground.tsx`
- `src/components/Layout/SessionSidebar.tsx`
- `src/components/RightSidebar/RightSidebar.tsx`
- `src/components/Layout/SplitView.tsx`
- `src/components/Terminal/Terminal.tsx`
- `src/components/Terminal/TerminalView.tsx`
- `src/components/SendBar/SendBar.tsx`
- `src/components/SendBar/TargetBar.tsx`
