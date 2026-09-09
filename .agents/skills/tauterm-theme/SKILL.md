---
name: tauterm-theme
description: "Single source of truth for TauTerm Liquid Glass UI, four-color ambient spectrum, clear-glass physics, theme tint veils, structural panels, control states, split/session presentation, motion, and rendering-performance rules."
license: MIT
metadata:
  author: tauterm
  version: "9.11"
---

# TauTerm Liquid Glass v9.11 — 唯一主题规范源

> **SSOT**：TauTerm 的主题、材质、四色环境色谱、Liquid Glass Physics、Theme Veil、Structural Panel、SendBar、SplitView 视觉状态与渲染性能规则只在本文件维护。  
> `docs/` 不复制主题规则；`tauterm-theme-review` 只维护审查流程。

## 1. 设计模型

视觉系统由四个正交层组成：

1. **四色环境 Ambient**：提供低频颜色与流动。
2. **Clear Liquid Glass Physics**：所有主题共享的透明玻璃本体。
3. **Theme Veil**：炫彩流光 / Obsidian / Frosted 只在同一玻璃上覆盖透明 / 黑 / 白薄膜。
4. **Performance Mode**：只改变 Ambient 动态成本与小面积 backdrop sampling；不改变材质身份、四色完整度或主题关系。

核心原则：

- **先有同一种玻璃，再有主题染膜。**
- Spectrum Flow 不是深蓝玻璃；它是三主题里最清、最亮、最透的一层。
- Obsidian 不是另一套厚重黑面板；它是同一透明玻璃 + 黑色 veil。
- Frosted 不是实心白卡片；它是同一透明玻璃 + 乳白 veil。
- 大面积工作区不依赖实时 backdrop blur。
- 同级结构必须复用同一个 surface class，不允许组件私建相似材质。

理想观感：清澈、柔亮、边缘有高光、内部只有轻微雾化，背景颜色可以穿透，但文字与控件仍稳定可读。

---

## 2. Canonical 四色环境色谱

唯一四色锚点：

- `--spectrum-red: #FE3734`
- `--spectrum-yellow: #F4BA00`
- `--spectrum-green: #02BE66`
- `--spectrum-blue: #0B8AFF`

二维空间固定：

- 左上 Red
- 左下 Yellow
- 右下 Green
- 右上 Blue

`--spectrum-gradient` / `--spectrum-gradient-soft` 必须一次同时呈现完整四色。Purple / Orange 只能由四色插值产生。

**四色完整度是不变量**：3 themes × 2 performance modes 都必须同时保留 Red / Yellow / Green / Blue 四个锚点。性能档只能改变运动、采样和合成成本，禁止删色、合并成单色场或让任一颜色在正常窗口中不可见。

---

## 3. Clear Liquid Glass Physics

### 共享物理层

所有主题必须共用：

- `--liquid-clear-shell-fill`
- `--liquid-clear-panel-fill`
- `--liquid-clear-content-fill`
- `--liquid-clear-card-fill`
- `--liquid-clear-control-fill`
- `--liquid-specular-shell-fill`
- `--liquid-specular-panel-fill`
- `--liquid-specular-content-fill`

这些 token 决定透明玻璃本体的亮边、轻雾、高光方向与内部反射，**不得按主题复制三份 physics**。

### Theme Veil

每个主题用以下 veil 建立视觉身份；允许对 border/shadow 做最低限度的对比补偿：

- `--theme-shell-veil`
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

### Spectrum Flow / 炫彩流光

- 三主题中 **veil 最弱**，Ambient 本身与其它主题完全同源、同强度。
- Ambient 能明显穿过 Sidebar / SendBar / Dialog。
- Structural Panel 不应看起来像红蓝绿黄实心色块；颜色来自背后的 Ambient。
- 保持中性清透，不加 Navy Blue 固有底色。
- 目标是 airy / luminous / clear / prismatic；基础中性明度必须高于 Obsidian，而不是仅靠 Ambient 颜色制造差异。
- **Performance 模式也必须保持明显的明亮烟晶身份**：可以用更高 RGB 明度的单层半透明 fill，但不要通过大幅降低 alpha 来提亮，否则无 blur 时容易出现后层文字穿透。
- Spectrum Flow 与 Obsidian 必须在正常截图里一眼能区分；在 Performance 模式下同样不接受“只是稍微亮一点”的差异。

### Obsidian / 黑曜石

- 与 Spectrum Flow 完全相同的 clear physics。
- 只通过更厚的黑色 veil 建立身份。
- Panel/Shell Surface 必须明显更黑，Content/Control 可以更实，以保证黑曜石的深邃感。
- 黑膜仍要保留少量 Ambient 和 specular，不能退化成完全不透明的黑卡片。

### Frosted / 白霜

- 与另外两主题完全相同的 clear physics。
- 只增加乳白 veil。
- 保留亮顶部 rim 与微弱暗下缘。
- 禁止退化成纯白纸片或塑料面板。

---

## 5. 四色环境 Ambient Flow

始终使用两个 oversized Field，**所有主题、所有性能档都保留这两层**：

- Field A：Red（左上） + Green（右下）
- Field B：Blue（右上） + Yellow（左下）

Ambient 的几何、颜色和强度是跨主题共享源；主题不得定义自己的 `--ambient-opacity-*`。主题差异只能来自底色 / Theme Veil / 对比补偿。

动态档只动画 transform：translate + 轻 rotation + 轻 scale。

- **效果优先 / Quality**：Field A / Field B 两层都独立动画，约 13s / 16s；横向路径可达约 ±18–21vw、纵向约 ±10–13vh。色团使用更大的柔光半径，两组对角光场运动中必须出现明显交叠
- **性能优先 / Performance**：两层都保留完整四色，但全部静态，不产生持续 Ambient transform 合成
- 为了放大运动感，优先把色团中心向内收、适度增大柔光半径、让 gradient 边缘在 raster 边界前归零，再扩大 transform 路径；**不得仅为了飘得更远而扩大 Ambient raster layer**
- 正常观察 3–5 秒必须能感知效果优先的明显位移与交叠
- layout drag / resize 时暂停纯装饰 Ambient，释放后恢复
- hidden 时暂停；仅 unfocused 不暂停
- reduced motion 停止效果优先的装饰动画；设置页必须实时显示系统 motion 偏好，不能让用户在效果优先静止时不知道原因

禁止 `filter: blur()`、`mix-blend-mode`、持续 background animation、常驻 `will-change`。

---

## 6. Surface 体系

### Small Shell Surface — `.liquid-glass`

Toolbar、Settings、Dialog、Command Palette 等小面积 shell surface。

- 可按 performance 使用 backdrop sampling。
- 使用 Clear Glass Physics + `--theme-shell-veil`。

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

滚动表格同样遵守单 surface owner：当外层 table wrapper 已负责 border / radius / fill / clipping / scroll 时，内部 table 不得再画第二套外边框和背景；独立 table 才自行拥有 surface。

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

- `SplitView.paneSurface` 是命名为 `session-pane` 的 **size CSS container**；custom content 的宽度与高度适配都必须以真实 Pane 为基准，而不是以 app/window viewport 为基准。
- TFTP / iperf / TRDP 等 custom content 优先使用 `@container session-pane (...)` 完成重排；**不得为了纯视觉/布局响应再给每个插件增加 ResizeObserver 或轮询**。
- 当 Pane 变短时，允许进入紧凑密度，但不能隐藏功能：优先减小空白、卡片 padding、表格空状态高度，并利用已有横向空间减少不必要的纵向堆叠。
- custom Session 的根视图负责自己的滚动边界；Split Pane surface 不再额外制造同轴外层滚动。确需内部滚动（日志、传输列表、长表格）时必须限定为明确的数据区域。
- 窄 Pane 可以把复杂对象编辑器堆叠为单列、把局部区域改为横向滚动；但简单设置区在“窄且短”的 2×2 Pane 中应优先利用两列，避免为了宽度适配反而制造过长的纵向页面。
- 所有关键按钮/表单在 1 / 2 / 4 Pane 与拖动后的短 Pane 中都必须可达。

---

## 10. 四色棱镜按钮

高价值 Primary / Selected 可使用完整四色 Prism；普通按钮不全息化。

- **效果优先 / Quality**：四色 Prism 必须缓慢流动。实现只允许小面积 pseudo layer 的 **transform-only** 动画；不得通过持续 background-position、gradient 参数或 filter 动画制造流动，避免每帧重绘。
- **性能优先 / Performance**：保持当前静态四色 Prism，不创建持续动画层。
- 任意时刻都要同时保留 Red / Yellow / Green / Blue 四个锚点；流动只能改变它们在按钮内部的位置关系，不能退化成单色扫光。
- 动画应是低频连续流动，不做高频闪烁；默认约 8s 一轮即可。
- 系统 reduced-motion 时静态；layout drag / resize 与 hidden/paused 状态下暂停。
- Hover：普通动作按钮**不换色**，只允许 lift、scale、edge/shadow 增强。
- **导航 Tab / 模式切换条 / 互斥筛选条例外**：同一 selector strip 内所有按钮的外部几何必须恒定；selected/hover 只能改变颜色、边缘和阴影，禁止 translate / scale 让当前项看起来更高或更宽。
- 紧凑 selector 统一使用全局 `.liquid-selector-strip` + `.liquid-selector-button`：高度与 Select 共用 `--select-height`，padding / font / line-height 由主题层拥有；组件 CSS 只能声明 flex/grid 占位、换行和最小宽度，不得再定义另一套按钮几何。
- **窄 Sidebar 的一级工具/类别导航不得通过横向滚动 Tab 条解决溢出。** 当一级类别不能稳定在一行内完整展示时，必须优先使用统一 `.liquid-glass-input.liquid-glass-select`；横向滚动只属于内容浏览，不属于一级导航交互。二级、少量且需要高频切换的互斥选项仍可使用 selector strip。
- **Native Select 主题合同**：所有原生 `<select>` 必须同时使用 `.liquid-glass-input + .liquid-glass-select`，组件不得把 `.liquid-control-surface` 直接当作 select 皮肤。展开后的 option popup 在桌面 WebView/系统原生层中可能由系统绘制，因此 `.liquid-glass-select` 必须声明与当前主题一致的 `color-scheme`：Spectrum Flow / Obsidian = dark，Frosted = light；同时保留 `--select-option-bg` 作为可 CSS 绘制路径的回退。Select 的 `padding-right` 必须在 padding shorthand 之后声明，确保主题箭头不会压住文字。
- **窄 Sidebar 的浮动状态/任务条必须保证关键操作始终可达。** 优先级固定为“取消/关闭操作 > 文件/任务身份 > 核心状态/百分比 > 进度可视化 > 辅助速率”。当宽度不足时使用现有 CSS size container + `@container` 重排/隐藏低优先级信息，不得把操作按钮推出裁剪区，也不得为此引入横向滚动、ResizeObserver 或 JS 宽度轮询。
- Active：普通动作按钮可轻微压下；selector strip 不改变外部尺寸。
- Disabled：使用统一 disabled surface，不保留动态 Prism。

---

## 11. Dialog Action Hierarchy

弹窗必须区分“业务决策选项”和“退出弹窗”两种语义，不能为了排版方便把所有按钮做成同权网格：

- 两按钮确认框：Cancel 是次要动作，Confirm/Save/Delete 是主动作；沿用右对齐 footer。
- 三个及以上互斥业务决策（例如文件冲突 Replace / Keep Both / Skip Existing）：业务决策组成独立 option group；Cancel 单独位于 footer，视觉与语义都不是第四个平级选项。
- 有破坏性的决策使用 danger 语义；推荐的无损决策可使用 primary；其它决策使用 secondary。默认焦点优先落在最安全的无损决策，危险操作不得默认获焦。
- Dialog 必须复用 `.liquid-glass` 外壳和公共 GlassButton/全局按钮材质；禁止组件私建另一套弹窗背景/按钮玻璃。
- 标准 Dialog 使用 `--radius-xl`；常规 padding 使用 `--spacing-xl`，窄窗口可降为 `--spacing-lg`。标题统一 `--text-md` + 700，正文统一 `--text-sm`，辅助/元信息统一 `--text-xs`；标准动作按钮使用 `GlassButton size="md"`，不得在单个弹窗里另写近似字号/按钮体系。
- 两按钮确认框的默认焦点必须落在 Cancel/安全动作；危险动作不得默认获焦。Tab 在弹窗动作内循环，Esc 取消。
- 禁止使用原生浏览器 `alert()/confirm()/prompt()` 作为产品 UI。非阻塞错误/提示使用全局 themed Toast；需要用户决策或授权的流程使用主题 Dialog/InlinePrompt。
- option group 在窄窗口改为纵向；不得为了保持多列把文案挤成难读的等宽小按钮。

## 12. Control Contrast

禁止整体 disabled opacity。

统一使用：

- `--control-disabled-bg`
- `--control-disabled-input-bg`
- `--control-disabled-border`
- `--control-disabled-text`
- `--control-disabled-icon`

Input 需要稳定凹槽和清楚 border；focus 只允许克制 ring。

---

## 13. Backdrop / 性能红线

`backdrop-filter` 只允许 Small Shell Surface / Float 在 `src/styles/global.css` 使用。

禁止：

- Sidebar / SendBar / TargetBar / Terminal / SplitView 大面积 backdrop
- CSS Module 私建 backdrop
- `transition: all`
- 大面积 `filter: blur`
- `mix-blend-mode`
- 持续 gradient/background animation（唯一例外：效果优先下小面积四色 Prism 按钮可使用 transform-only pseudo layer 流动；仍禁止动画 gradient/background 本身）
- 常驻 `will-change`
- 无条件 `translateZ(0)`
- 纯装饰用途持续 layout polling / ResizeObserver

---

## 14. Performance

视觉性能只保留两个明确档位：**效果优先 / Quality** 与 **性能优先 / Performance**。禁止重新引入没有显著 GPU/流畅度收益的中间档。

效果优先 / Quality：
- 2 dynamic Ambient Fields，完整四色
- 两层独立大范围 transform，红/绿与蓝/黄光场在中心区域明显交叠
- 色团尺寸比旧实现更大，但 Ambient raster layer 尺寸保持不变
- Small Shell Surface / Float 使用更高 blur sampling 与 saturate
- 高价值四色 Prism 按钮使用小面积 transform-only 流动层
- 大面积 Structural / Content 仍然不做 backdrop sampling
- 目标：最大化液态玻璃层次与流动感

性能优先 / Performance：
- 2 static Ambient Fields，完整四色同时可见
- backdrop blur = 0，saturate sampling = 0
- 完全停止 Ambient 装饰动画
- 四色 Prism 按钮保持静态，不创建持续按钮动画层
- **所有主要 surface 退化为单层 flat translucent fill**：Shell Surface / Panel / Content / Control / Card / Float 分别只引用一个 `--performance-*-fill`
- 不叠 specular + clear + veil 多层背景，不保留大面积 glass shadow；现有 1px 结构 border 可以保留
- flat translucent fill 必须足够实，不能让后层文字形成清晰重影；炫彩流光使用更明亮的中性烟晶 fill，黑曜石使用深黑 fill，白霜使用浅色 fill
- 性能优先下的主题区分优先靠 **fill 自身明度 / 中性色相**，而不是降低 alpha 或增加额外特效；这不会破坏低成本路径
- 目标：最低持续 GPU 开销与最高交互流畅度，同时保持三主题一眼可分

交互降载：
- Sidebar / SendBar / Split divider 等布局拖动期间，暂时关闭 Small Shell Surface / Float backdrop sampling，并暂停 Ambient 装饰动画
- mouseup / cancel 后立即恢复当前性能档材质与动画状态

---

## 15. 提交前审计

```bash
rg 'backdrop-filter' src/components src/renderers --glob '*.module.css'
rg 'transition:\s*all' src --glob '*.css'
rg 'filter:\s*blur|mix-blend-mode|will-change' src --glob '*.css' --glob '*.tsx'
rg -U ':disabled[^\{]*\{[^\}]*opacity\s*:\s*0\.' src --glob '*.css'
rg 'liquid-glass-panel|liquid-glass-content|liquid-control-surface|liquid-glass-float|liquid-glass' src --glob '*.tsx'
rg 'theme-(shell|panel|content|card|float|control).*veil|performance-(shell|panel|content|control|card|float)-fill|liquid-clear-|liquid-specular-' src/styles
rg 'paneFrame|selectedFrame|dockedBorderRadii|pane(Frame|Header|Content)Radius' src/components/Layout src/components/Terminal
rg 'liquid-glass-content' src/components/Layout/SplitView.tsx src/components/Terminal/TerminalView.tsx
rg 'selectedHeader|content-divider|scrollbar-(button|corner)|requestAnimationFrame|onMouseDownCapture|container-name' src/components/Layout src/components/Terminal src/styles
rg '@container\s+session-pane|@media\s*\(max-width' src/components src/plugins --glob '*.module.css'
rg 'modeHeader|modeTitle|flex-direction:\s*row' src/components/SendBar
rg '#FE3734|#F4BA00|#02BE66|#0B8AFF|#4285F4|#EA4335|#FBBC05|#34A853' src --glob '*.css' --glob '*.tsx' --glob '*.ts'
npm run build
```

---

## 16. 视觉验收

至少覆盖：

- 3 themes × 2 performance modes
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
- **炫彩流光明显最透亮，Ambient 穿透最清楚；性能优先下也必须保持明亮烟晶观感，不能收敛成接近黑曜石的近黑表面**
- **黑曜石 = 同一 clear glass + black base / 更厚 black veil，必须明显比炫彩流光更黑**
- **白霜 = 同一 clear glass + white base / white veil**
- **三个主题都有清澈、柔亮、边缘高光的液态玻璃质感**
- **单 Pane 没有 active/selected 材质差异**
- **中间 Workspace 只有一层 Content 外框/阴影；分屏后不出现 Pane 套 Card 的第二层框**
- **多 Pane 只有 Workspace root 拥有外角，Pane 子矩形与内部交点保持直角**
- **selected Pane 只有一条 1px Header accent，不出现双线/内阴影边**
- **内部 Divider 比 Workspace 外框更弱，hover 才进入 accent；滚动条两端没有原生箭头按钮，横纵滚动条交汇处没有白色 corner 方块**
- **右键未选中 Pane Header 或已连接 Terminal 时都不会先切换 active Session；Close Pane 菜单只从 Header 出现**
- **窄 Sidebar 的一级工具导航不出现横向滚动条；类别较多时使用主题 Select，滚动条只用于内容区域**
- **文件管理器传输状态条在常规窄 Sidebar 中仍显示真实传输速度并能直接点击取消/关闭；只有 <=220px 的极窄档才可隐藏速度/进度，关键操作不得被裁掉**
- **展开任意主题 Select 时，原生 option popup 的明暗必须与当前主题一致，不得出现深色主题白底白字/浅字菜单**
- **文件冲突弹窗的 Replace / Keep Both / Skip Existing 是业务决策组，Cancel 独立位于 footer；危险/推荐/普通动作层级清楚，不出现四个等权按钮的 2×2 网格**
- **Divider 拖动每动画帧最多提交一次布局更新，释放鼠标后最终 ratio 不丢失**
- **效果优先正常观察 3–5 秒能看出两层 Ambient 明显位移与交叠；四色 Prism 按钮也能感知低频连续流动；色团更大但 raster layer 不扩大**
- **系统 reduced-motion 生效时，设置页必须明确显示“系统动态效果已关闭/减少动态效果”，并解释效果优先因此静止**
- **性能优先保留两层完整四色但静态；四色 Prism 按钮也必须静态；主要 surface 是单层半透明纯色，没有 backdrop、specular 多层和大面积 glass shadow**
- **性能优先的后层文字不能形成清晰重影，持续 GPU 开销必须显著低于效果优先**

## 实现源文件

- `src/styles/tokens.css`
- `src/styles/global.css`
- `src/context/ThemeContext.tsx`
- `src/App.tsx`
- `src/components/Layout/SpectrumAmbientBackground.tsx`
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
