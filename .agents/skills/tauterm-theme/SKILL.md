---
name: tauterm-theme
description: "Single source of truth for TauTerm Liquid Glass UI, Gemini/Google ambient spectrum, clear-glass physics, theme tint veils, structural panels, control states, split/session presentation, motion, and rendering-performance rules."
license: MIT
metadata:
  author: tauterm
  version: "8.7"
---

# TauTerm Liquid Glass v8.7 — 唯一主题规范源

> **SSOT**：TauTerm 的主题、材质、Gemini 色谱、Liquid Glass Physics、Theme Veil、Structural Panel、SendBar、SplitView 视觉状态与渲染性能规则只在本文件维护。  
> `docs/` 不复制主题规则；`tauterm-theme-review` 只维护审查流程。

## 1. 设计模型

视觉系统由四个正交层组成：

1. **Gemini Ambient**：提供低频颜色与流动。
2. **Clear Liquid Glass Physics**：所有主题共享的透明玻璃本体。
3. **Theme Veil**：Google Glow / Obsidian / Frosted 只在同一玻璃上覆盖透明 / 黑 / 白薄膜。
4. **Performance Mode**：只改变 Ambient 动态成本与小面积 backdrop sampling；不改变材质身份、四色完整度或主题关系。

核心原则：

- **先有同一种玻璃，再有主题染膜。**
- Google Glow 不是深蓝玻璃；它是三主题里最清、最亮、最透的一层。
- Obsidian 不是另一套厚重黑面板；它是同一透明玻璃 + 黑色 veil。
- Frosted 不是实心白卡片；它是同一透明玻璃 + 乳白 veil。
- 大面积工作区不依赖实时 backdrop blur。
- 同级结构必须复用同一个 surface class，不允许组件私建相似材质。

理想观感：清澈、柔亮、边缘有高光、内部只有轻微雾化，背景颜色可以穿透，但文字与控件仍稳定可读。

---

## 2. Canonical Gemini Spectrum

唯一品牌色：

- `--google-red: #FE3734`
- `--google-yellow: #F4BA00`
- `--google-green: #02BE66`
- `--google-blue: #0B8AFF`

二维空间固定：

- 左上 Red
- 左下 Yellow
- 右下 Green
- 右上 Blue

`--google-brand-gradient` / `--google-brand-gradient-soft` 必须一次同时呈现完整四色。Purple / Orange 只能由四色插值产生。

**四色完整度是不变量**：3 themes × 3 performance modes 都必须同时保留 Red / Yellow / Green / Blue 四个锚点。性能档只能改变运动、采样和合成成本，禁止删色、合并成单色场或让任一颜色在正常窗口中不可见。

---

## 3. Clear Liquid Glass Physics

### 共享物理层

所有主题必须共用：

- `--liquid-clear-chrome-fill`
- `--liquid-clear-panel-fill`
- `--liquid-clear-content-fill`
- `--liquid-clear-card-fill`
- `--liquid-clear-control-fill`
- `--liquid-specular-chrome-fill`
- `--liquid-specular-panel-fill`
- `--liquid-specular-content-fill`

这些 token 决定透明玻璃本体的亮边、轻雾、高光方向与内部反射，**不得按主题复制三份 physics**。

### Theme Veil

每个主题用以下 veil 建立视觉身份；允许对 border/shadow 做最低限度的对比补偿：

- `--theme-chrome-veil`
- `--theme-panel-veil`
- `--theme-content-veil`
- `--theme-content-active-veil`
- `--theme-content-inactive-veil`
- `--theme-content-header-veil`
- `--theme-card-veil`
- `--theme-float-veil`
- `--theme-control-veil`

Surface 的背景组合顺序固定为：

1. specular
2. theme veil
3. clear glass base

禁止重新回到“每个主题各写一套完整 panel/content gradient”的模式。

---

## 4. Theme Identity

### Google Glow / 炫彩流光

- 三主题中 **veil 最弱**，Ambient 本身与其它主题完全同源、同强度。
- Ambient 能明显穿过 Sidebar / SendBar / Dialog。
- Structural Panel 不应看起来像红蓝绿黄实心色块；颜色来自背后的 Ambient。
- 保持中性清透，不加 Navy Blue 固有底色。
- 目标是 airy / luminous / clear / prismatic。
- Google Glow 与 Obsidian 必须在正常截图里一眼能区分，不接受“只是稍微亮一点”的差异。

### Obsidian / 黑曜石

- 与 Google Glow 完全相同的 clear physics。
- 只通过更厚的黑色 veil 建立身份。
- Panel/Chrome 必须明显更黑，Content/Control 可以更实，以保证黑曜石的深邃感。
- 黑膜仍要保留少量 Ambient 和 specular，不能退化成完全不透明的黑卡片。

### Frosted / 白霜

- 与另外两主题完全相同的 clear physics。
- 只增加乳白 veil。
- 保留亮顶部 rim 与微弱暗下缘。
- 禁止退化成纯白纸片或塑料面板。

---

## 5. Gemini Ambient Flow

始终使用两个 oversized Field，**所有主题、所有性能档都保留这两层**：

- Field A：Red（左上） + Green（右下）
- Field B：Blue（右上） + Yellow（左下）

Ambient 的几何、颜色和强度是跨主题共享源；主题不得定义自己的 `--ambient-opacity-*`。主题差异只能来自底色 / Theme Veil / 对比补偿。

动态档只动画 transform：translate + 轻 rotation + 轻 scale。

- Balanced：约 20s / 24s，低频连续流动
- Quality：约 12s / 15s，路径更明显、流动更易感知
- 正常观察 3–5 秒必须能感知 Quality / Balanced 的位置变化
- Compat：两层都保留，完整四色同时可见，但静态
- layout drag / resize 时暂停纯装饰 Ambient，释放后恢复
- hidden 时暂停；仅 unfocused 不暂停
- reduced motion 停止装饰动画

禁止 `filter: blur()`、`mix-blend-mode`、持续 background animation、常驻 `will-change`。

---

## 6. Surface 体系

### Small Chrome — `.liquid-glass`

Toolbar、Settings、Dialog、Command Palette 等小面积 chrome。

- 可按 performance 使用 backdrop sampling。
- 使用 Clear Glass Physics + `--theme-chrome-veil`。

### Structural Panel — `.liquid-glass-panel`

**唯一适用对象：**

- 左 Sidebar 外壳
- 右 Sidebar 外壳
- SendBar 主外壳
- TargetBar

这四者必须：

- 使用同一个全局 class；
- 使用同一组 `--panel-*` edge/shadow token；
- 使用同一 `--liquid-clear-panel-fill`；
- 只通过当前主题的 `--theme-panel-veil` 染色；
- 不做 backdrop-filter；
- 每个结构只出现一层 panel glass，禁止嵌套重复 glass。

因此在同一主题下，左栏、右栏、SendBar、TargetBar 的**边框、阴影、透明材质完全同源**。

### Content — `.liquid-glass-content`

Terminal、Network/TFTP/iperf 主内容、空 Pane。

Content 与 Structural Panel **必须同宗**：

- 同样由 clear base + specular + veil 组合；
- specular 更弱；
- veil 更稳定；
- 阴影更轻；
- 不使用 backdrop-filter。

终端不应复制 Structural Panel 的强壳体感，否则会增加视觉噪音；它是“同一玻璃的内容级版本”。

### Control Surface — `.liquid-control-surface`

高密度表单/参数编辑内部区域。仍使用 clear control base + theme control veil，但 veil 可以更实以确保可读性。

### Float / Card / Accent

Float、Card、Accent 也必须从同一 clear physics 派生，不能各自造玻璃渐变。

---

## 7. SendBar — 布局冻结

用户已明确：**不要修改发送栏整体布局。**

四模式：

- Basic
- Command
- Auto Reply
- Script

固定结构：

- **左侧一竖排四个模式切换按钮**
- 右侧内容区
- TargetBar 在需要时位于主体上方
- 四个子视图保持 mounted，CSS 切换显示

禁止：

- 把四模式改成顶部横排；
- 为模式增加顶部 toolbar；
- 为了材质优化改动信息架构或控件位置。

允许：

- 当前模式使用 `.liquid-theme-selected`；
- 左侧模式列作为 Structural Panel 内部轻分区；
- 使用 `--panel-divider` / `--panel-subsection-fill`；
- 调整透明度、高光、边缘，但不得改变布局。

`--sendbar-min-height` 必须由四个竖排按钮的高度、gap、padding 推导。

---

## 8. Sidebar / Structural Consistency

左 Sidebar、右 Sidebar、SendBar、TargetBar：

- App/组件层各只能有一层 `.liquid-glass-panel`；
- radius / border / top-left specular / shadow / transparency 完全来自同一 Structural Panel；
- 内部列表项默认扁平；
- 不为装饰渐隐或阴影引入持续 ResizeObserver。

如果肉眼看到左栏和右栏像两种材料，视为实现错误。

---

## 9. Terminal / SplitView

### Workspace Surface Ownership

Split Workspace 只能有一层 Content 壳体：

- `SplitView` root 是唯一的基础 `.liquid-glass-content` owner，统一负责 Workspace 的 background / border / shadow / radius。
- Pane 内容与 docked Terminal wrapper **不得再次附加基础 `.liquid-glass-content`**；多 Pane 只允许叠加 active / inactive veil state。
- Workspace root 统一裁剪四个真实外角；Pane 子矩形保持直角，不再维护 pane-local corner geometry。
- 内部边界只由 Pane Header 的分隔线和 Divider 表达；禁止恢复 `paneFrame` / selected perimeter frame。
- selected 状态只通过 Header 的克制 accent / veil 表达，不给中间内容区再套一层边框或阴影。

这样 Terminal 仍是 Content 材质，但一个 Workspace 只付出一次外壳边框/阴影成本，也不会出现“分屏后又套了一层框”的视觉噪音。

### 单 Pane

只有一个 Pane 时：

- 不显示 Pane Header；
- 不显示 selected frame / selected material；
- Workspace root 使用 neutral `.liquid-glass-content`；
- Terminal / custom content 本体透明到这一个 root surface。

### 多 Pane

只有 `paneCount > 1` 时：

- selected pane 可叠加 active veil；
- 其它 pane 可叠加 inactive veil；
- Pane Header 使用 `--theme-content-header-veil` + 同一 content clear base；
- selected Header 只允许 **一条 1px accent 分隔线**；禁止再叠加 inset shadow / 第二条高亮线；
- Divider 是 1px 语义分隔线 + 更宽 hit-zone，不是第二层 Card 边框；idle 必须使用主题级 `--content-divider`，视觉强度低于 Workspace 外框，hover 才提升为 accent；
- 所有 Pane 内部交点保持直角，外角由 Workspace root 的 overflow clipping 自动完成；
- WebKit 滚动条只保留 track + thumb：必须全局隐藏原生 `::-webkit-scrollbar-button`，并将 `::-webkit-scrollbar-corner` 设为透明；横纵滚动条同时出现时，右下交汇处不得出现原生白色方块。

### Context / Interaction Stability

- `Close Pane` 右键菜单只属于 **Pane Header**；Pane content 绝不打开 Pane-level close menu。
- 对未选中的 Pane Header 按下右键时，**不得先激活该 Pane**。必须等 `contextmenu` 在原几何位置打开菜单，避免 SendBar / RightSidebar 切换导致标题栏在指针下发生位移。
- docked Terminal 也遵循同一规则：**只有 primary-button / 左键**可以激活 Pane；secondary-button / 右键必须保留原 active Session，直接交给 Terminal context menu。
- 左键点击 Header / content 才执行 Pane selection。
- Divider resize / Pane geometry 改变不得对 `left/top/width/height` 做 CSS transition；拖动必须直接跟手。
- Divider 高频 mousemove 必须按 `requestAnimationFrame` 合并为每帧最多一次 ratio 更新，并在 mouseup / window blur 时提交最后 pending ratio、清理 listener/cursor。

### Disconnected

Network Debug 与其它会话共用 SplitView 的 `PaneEmptyState`。所有 disconnected Session（terminal / custom）都允许在内容区打开 Session-level Connect / Configure / Delete 等动作，但不得借用 Pane-level `Close Pane` 菜单。

### Pane-relative Adaptive Content

- `SplitView.paneSurface` 是命名为 `session-pane` 的 inline-size CSS container；custom content 的窄宽度适配必须以真实 Pane 为基准，而不是以 app/window viewport 为基准。
- TFTP / iperf / TRDP 等 custom content 优先使用 `@container session-pane (...)` 完成重排；**不得为了纯视觉/布局响应再给每个插件增加 ResizeObserver 或轮询**。
- 当 Pane 变短时，交互控件必须始终可达：负责滚动的容器要明确，包含按钮/表单的 flex 子项不得因默认 shrink + `overflow: hidden` 被压扁裁切。
- 窄 Pane 可以把多列内容堆叠为单列、把局部区域改为横向滚动，但不能通过隐藏关键控件来“适配”。
- Pane 自身已有滚动时，插件避免制造无必要的同轴多层滚动；确需内部滚动（日志、传输列表、长表格）时必须限定为明确的数据区域。

---

## 10. Gemini Prism Button

高价值 Primary / Selected 可使用完整四色 Prism。

Idle：完整四色同时可见。  
Hover：**不换色**，只允许 lift、scale、edge/shadow 增强。  
Active：轻微压下。

普通按钮不全息化。

---

## 11. Control Contrast

禁止整体 disabled opacity。

统一使用：

- `--control-disabled-bg`
- `--control-disabled-input-bg`
- `--control-disabled-border`
- `--control-disabled-text`
- `--control-disabled-icon`

Input 需要稳定凹槽和清楚 border；focus 只允许克制 ring。

---

## 12. Backdrop / 性能红线

`backdrop-filter` 只允许 Small Chrome / Float 在 `src/styles/global.css` 使用。

禁止：

- Sidebar / SendBar / TargetBar / Terminal / SplitView 大面积 backdrop
- CSS Module 私建 backdrop
- `transition: all`
- 大面积 `filter: blur`
- `mix-blend-mode`
- 持续 gradient/background animation
- 常驻 `will-change`
- 无条件 `translateZ(0)`
- 纯装饰用途持续 layout polling / ResizeObserver

---

## 13. Performance

三个档位是一条单一维度：**效果丰富度 / GPU 成本**。它们不得改变主题身份，也不得删减四色。

Quality：
- 2 dynamic Ambient Fields，完整四色
- 更明显的 transform 路径与更快的低频流动
- Small Chrome / Float 使用更高 blur sampling 与适度更高 saturate
- 大面积 Structural / Content 仍然不做 backdrop sampling

Balanced：
- 2 dynamic Ambient Fields，完整四色
- 默认约 20s / 24s motion
- Small Chrome / Float 使用 8–10px 级轻量 sampling
- 默认推荐档

Compat：
- 2 static Ambient Fields，完整四色同时可见
- backdrop blur = 0，saturate sampling = 0
- 停止纯装饰 Ambient 合成动画，这是兼容档最主要的 GPU 降级来源之一
- Structural / Content 保持正常 clear base + theme veil，不额外变厚
- **依赖 backdrop 的 Small Chrome / Float 必须切换到更实的 `--theme-*-veil-compat` fallback**，避免底层文字和边框无模糊地直接穿透
- Compat 必须感知上比 Balanced 更稳、更实，绝不能出现“关闭 blur 后反而更透明、更玻璃、文字重叠更严重”的倒挂
- Compat 仍是 Liquid Glass，不得退回死板的完全 opaque surface

交互降载：
- Sidebar / SendBar / Split divider 等布局拖动期间，暂时关闭 Small Chrome / Float backdrop sampling，并暂停 Ambient 装饰动画
- mouseup / cancel 后立即恢复当前性能档材质与动画状态

---

## 14. 提交前审计

```bash
rg 'backdrop-filter' src/components src/renderers --glob '*.module.css'
rg 'transition:\s*all' src --glob '*.css'
rg 'filter:\s*blur|mix-blend-mode|will-change' src --glob '*.css' --glob '*.tsx'
rg -U ':disabled[^\{]*\{[^\}]*opacity\s*:\s*0\.' src --glob '*.css'
rg 'liquid-glass-panel|liquid-glass-content|liquid-control-surface|liquid-glass-float|liquid-glass' src --glob '*.tsx'
rg 'theme-(chrome|panel|content|card|float|control).*veil|theme-(chrome|float)-veil-compat|liquid-clear-|liquid-specular-' src/styles
rg 'paneFrame|selectedFrame|dockedBorderRadii|pane(Frame|Header|Content)Radius' src/components/Layout src/components/Terminal
rg 'liquid-glass-content' src/components/Layout/SplitView.tsx src/components/Terminal/TerminalView.tsx
rg 'selectedHeader|content-divider|scrollbar-(button|corner)|requestAnimationFrame|onMouseDownCapture|container-name' src/components/Layout src/components/Terminal src/styles
rg '@container\s+session-pane|@media\s*\(max-width' src/components src/plugins --glob '*.module.css'
rg 'modeHeader|modeTitle|flex-direction:\s*row' src/components/SendBar
rg '#FE3734|#F4BA00|#02BE66|#0B8AFF|#4285F4|#EA4335|#FBBC05|#34A853' src --glob '*.css' --glob '*.tsx' --glob '*.ts'
npm run build
```

---

## 15. 视觉验收

至少覆盖：

- 3 themes × 3 performance modes
- Settings / ConnectDialog
- 左右 Sidebar 同时可见
- SendBar Basic / Command / Auto Reply / Script
- single / 2 / 4 pane
- Terminal / Network disconnected
- TFTP / iperf / TRDP in short and narrow Panes; all controls reachable
- TargetBar visible / hidden
- Ambient 连续 ≥10s
- disabled controls 位于四色最亮区域

验收句：

- **发送栏四个模式仍在左侧竖排**
- **左栏、右栏、SendBar、TargetBar 是同一种 Structural Glass**
- **Terminal 是同一种玻璃的克制 Content 版本**
- **三主题使用同一份 Ambient 几何 / 色值 / 强度；差异只来自底色、Theme Veil 与必要的对比补偿**
- **炫彩流光明显最透亮，Ambient 穿透最清楚**
- **黑曜石 = 同一 clear glass + black base / 更厚 black veil，必须明显比炫彩流光更黑**
- **白霜 = 同一 clear glass + white base / white veil**
- **三个主题都有清澈、柔亮、边缘高光的液态玻璃质感**
- **单 Pane 没有 active/selected 材质差异**
- **中间 Workspace 只有一层 Content 外框/阴影；分屏后不出现 Pane 套 Card 的第二层框**
- **多 Pane 只有 Workspace root 拥有外角，Pane 子矩形与内部交点保持直角**
- **selected Pane 只有一条 1px Header accent，不出现双线/内阴影边**
- **内部 Divider 比 Workspace 外框更弱，hover 才进入 accent；滚动条两端没有原生箭头按钮，横纵滚动条交汇处没有白色 corner 方块**
- **右键未选中 Pane Header 或已连接 Terminal 时都不会先切换 active Session；Close Pane 菜单只从 Header 出现**
- **Divider 拖动每动画帧最多提交一次布局更新，释放鼠标后最终 ratio 不丢失**
- **Quality / Balanced 正常观察 3–5 秒能看出 Ambient 位置变化；Compat 保留两层完整四色但静态**
- **Compat 仍然是玻璃，但 Small Chrome/Float 的可读性不能低于 Balanced，也不能出现“兼容档最透明”**

## 实现源文件

- `src/styles/tokens.css`
- `src/styles/global.css`
- `src/context/ThemeContext.tsx`
- `src/App.tsx`
- `src/components/Layout/GoogleGlowBackground.tsx`
- `src/components/Settings/panels/AppearanceSettings.tsx`
- `src/i18n/locales/zh-CN.json`
- `src/i18n/locales/en-US.json`
- `src/components/Layout/SessionSidebar.tsx`
- `src/components/RightSidebar/RightSidebar.tsx`
- `src/components/Layout/SplitView.tsx`
- `src/components/Layout/SplitView.module.css`
- `src/components/Terminal/Terminal.tsx`
- `src/components/Terminal/TerminalView.tsx`
- `src/components/SendBar/SendBar.tsx`
- `src/components/SendBar/SendBar.module.css`
- `src/components/SendBar/TargetBar.tsx`
