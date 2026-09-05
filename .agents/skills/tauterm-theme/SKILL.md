---
name: tauterm-theme
description: "Single source of truth for TauTerm Liquid Glass UI, shared Google ambient glow, theme identity, material layering, visual performance tiers, and rendering-performance rules. Use for any React/CSS UI creation or modification, theme work, visual review/fixes, animation, glass/backdrop effects, or appearance/performance settings."
license: MIT
metadata:
  author: tauterm
  version: "5.0"
---

# TauTerm Liquid Glass v5 — 唯一主题规范源

> **SSOT**：本文件是 TauTerm 视觉材质、主题身份、Google Ambient、性能档与 UI 渲染性能规则的唯一规范源。  
> `tauterm-theme-review` 只能描述“如何审查”，不得复制或重新定义本文件中的规则。

## 目标

TauTerm 的 Liquid Glass 不是“大面积毛玻璃”。设计目标：

- **清透**：内容能感知到底层 Google 色光，但文字/数据始终占第一视觉层级。
- **统一品牌层**：Google 蓝/红/黄/绿环境光是 TauTerm 跨主题共享的视觉 DNA。
- **主题有独立身份**：主题通过底色、玻璃 tint、内容 surface、边缘高光、阴影和文字体系区分，而不是换一套背景动画。
- **性能档正交**：Performance 只控制效果丰富度和渲染成本，不改变主题身份。
- **边缘光学感**：通过 specular/rim light、薄边框、轻阴影表达玻璃厚度；不用增加大面积 blur。
- **老设备可用**：Balanced 是默认；Compatibility 在无高性能 GPU、软件渲染或远程桌面下仍保持完整配色和层次。

---

## 1. 四层材质语义

### A. Chrome — `.liquid-glass`

用于 Toolbar、SessionSidebar/RightSidebar 外壳、StatusBar、Dialog/Settings 主框架和小面积固定工具面。

- 可使用轻量 `backdrop-filter`。
- blur 由 `--glass-blur-chrome` + `data-performance` 控制。
- 使用 `--glass-specular-fill` + `--glass-fill` + rim/shadow。
- **禁止**用于 Terminal、文件列表、数据表、图表主体等大面积内容层。

### B. Content — `.liquid-glass-content`

用于 Terminal pane、File browser/data view、Network/TFTP/iperf/TRDP 主内容、可扩展 SendBar 主体、空 Pane/断开占位。

- **不得使用 `backdrop-filter`**。
- 使用稳定半透明 `--content-surface`。
- active / inactive 分别使用：
  - `.liquid-glass-content-active`
  - `.liquid-glass-content-inactive`
- active pane 必须比 inactive pane **更透**，用透明度而不是粗描边建立焦点。
- 背景 Google 色应像“玻璃后面的光”，不能把 pane 染成纯红/绿/黄。

### C. Accent — `.liquid-glass-accent`

用于 command/search trigger、active pill、segmented control、小型 hover/selected control。

- 默认不常驻 backdrop blur。
- 通过 tint、specular edge、rim light、短交互位移表达液态感。

### D. Float — `.liquid-glass-float`

用于 ContextMenu、Toast、Search dropdown、Popover 等 absolute/fixed/createPortal 浮层。

- 可使用 `--glass-blur-float`。
- Compatibility 自动关闭 backdrop blur。

### E. Nested cards

使用 `.liquid-glass-card` / `.liquid-glass-mini-card` / `.liquid-glass-status-card`。嵌套在 chrome/content 内部时不得再套一层 `.liquid-glass`。

---

## 2. Brand Ambient：三主题共享同一套 Google 四色流光

Google Ambient 是全局品牌层，不属于任何一个主题。

### 三主题必须完全一致的部分

- Google RGB：
  - Blue `#4285F4`
  - Red `#EA4335`
  - Yellow `#FBBC05`
  - Green `#34A853`
- 光团数量（同一 performance mode 下）
- 光团尺寸
- gradient stops / softness
- 初始 geometry
- 运动轨迹和方向
- 同一 performance mode 下的动画节奏

### 主题允许不同的唯一 Ambient 参数

主题可定义**感知强度补偿**，用于补偿不同底色：

- 深色主题：较低补偿
- 浅色 Frosted：允许更高 opacity，以抵消白底对半透明彩色光的冲淡

这不是换一套光团，而是同一品牌层在不同底色上的视觉校准。

### 禁止

- 某主题单独删除某种 Google 色（除 Compatibility 统一降级）
- 为 Obsidian 换成蓝紫专属背景
- 为 Frosted 换 pastel 专属 RGB
- 大面积 `filter: blur(...)`
- 100px+ 实时 blur
- 动画 `border-radius` / clip-path morph
- 全屏 `mix-blend-mode`
- 常驻 `will-change`

---

## 3. 三主题身份

| 主题 | 第一印象 | Base / Chrome | Content surface | Specular |
|---|---|---|---|---|
| `google-glow` 炫彩流光 | 活跃、通透、彩色 | 深蓝黑 / 靛蓝玻璃 | 三者中最透 | 最明显但保持薄 |
| `obsidian` 黑曜石 | 深邃、烟晶、专业 | 接近纯黑 / 石墨 | 三者中最实 | 最克制 |
| `frosted` 白霜 | 冰晶、明亮、轻盈 | 银白 / 冷白玻璃 | 中等通透 | 亮白/冰蓝边缘 |

### 规则

1. 三主题**共享同一 Google Ambient**。
2. 主题区分主要发生在材质而不是背景动画。
3. Google Glow 与 Obsidian 的静态截图必须在 1 秒内可分辨。
4. Frosted 不得退化成普通灰白 light theme；必须保留冷白玻璃与 Google 色透射感。

---

## 4. Visual Performance 是效果丰富度，不是主题

`ThemeContext` 在 `<html>` 写入 `data-performance`。

### `quality` — UI 中文“效果优先”

- 完整 4 色 Ambient。
- Ambient 强度最高。
- motion amplitude 最大。
- motion duration 最短，但仍是缓慢环境运动。
- 小面积 chrome/float 可使用更强 sampling。
- **Content backdrop blur 始终为 0。**

### `balanced` — 默认/推荐

- 完整 4 色 Ambient。
- Ambient 强度中等。
- motion amplitude 中等、节奏更慢。
- chrome/float sampling 较轻。
- **Content backdrop blur 始终为 0。**

### `compat` — 兼容

- 可统一减少为 2 个静态 Google 光团。
- 所有 ambient animation 停止。
- chrome / float backdrop blur = 0。
- 主题配色、content transparency hierarchy、specular/rim 仍保留。

### 正交原则

切 Performance：

- 可以改变 opacity / motion amplitude / duration / allowed chrome sampling。
- 不得改变主题 base color、content tint、文字体系、Google RGB。
- Quality 与 Balanced 静态截图可以相似，但运行 3–5 秒必须能感知 Quality 更活跃。
- Compatibility 必须显著静态、路径显著更轻。

**禁止通过 GPU 型号猜测自动切档。** 默认 Balanced，用户明确选择 Compat。

---

## 5. Ambient motion

`ThemeContext` 在 `<html>` 写入 `data-motion`：

- `full`
- `reduced`
- `paused`

规则：

1. Ambient 只能使用 **transform-only** animation。
2. Quality 目标周期约 24–30s；Balanced 约 34–42s。
3. Quality motion amplitude 目标约 16–22vw / 12–18vh；Balanced 约 10–16vw / 8–12vh。
4. 光核在运动期间必须进入 viewport，不得长期只把 gradient 边缘放在屏幕内。
5. 用户正常使用 3–5 秒内应能感知色场发生变化。
6. 窗口隐藏/失焦时 animation 必须暂停。
7. `prefers-reduced-motion` 下装饰动画停止。

---

## 6. Content transparency

Content surface 的职责是“稳定文字 + 保留背景光感”，不是遮住背景。

### 相对透明度

从更透到更实：

1. Google Glow active
2. Frosted active
3. Google Glow normal
4. Frosted normal
5. Obsidian active
6. inactive surfaces（各自在 normal 基础上更实）

### 设计边界

- Google Glow 必须能在 Terminal 中感知到 Google 色缓慢经过。
- Obsidian 仍能看到同一 Google Ambient，但应像隔着深色烟晶。
- Frosted 中 Google RGB 由白色基底自然混合成较浅色，不另造 pastel palette。
- 不通过重新开启 content backdrop blur 获得透明感。
- 不允许背景影响 terminal 字符可读性。

---

## 7. Modal overlay

所有 modal 使用统一 `.glass-overlay` 行为。

- **Appearance 不得有专门 Theme Preview Overlay。**
- 不因页面不同改变 modal 交互模型。
- 每个主题可通过 `--overlay-bg` 调整遮光强度：
  - Google Glow：中等偏轻，背景 Ambient 仍可辨
  - Obsidian：较深
  - Frosted：很轻
- overlay 只负责聚焦 modal，不应把当前主题身份完全抹掉。

---

## 8. Backdrop-filter 红线

`backdrop-filter` / `-webkit-backdrop-filter` 只能由 `src/styles/global.css` 的全局材质类实现。

**CSS Module 中出现 backdrop-filter 默认视为 HIGH/CRITICAL。**

尤其禁止：

- Terminal / xterm 外壳
- SplitView 大面积 pane
- FileManager 主体
- 图表/日志/数据表主体
- 可展开 SendBar
- glass 内嵌 glass

Compatibility 必须统一关闭所有允许的 backdrop sampling。

---

## 9. CSS / Token 规则

业务组件不得新增硬编码颜色。使用：

- `--text-*`
- `--accent-*`
- `--color-*`
- `--glass-*`
- `--content-*`
- `--ambient-*`

允许例外：

- Google 四色 Brand Ambient
- 永远为白色的 `#fff` on-accent 文本
- 已有、注明原因的 SVG data URI 色值

任何新增材质 token 必须同时在 google-glow / obsidian / frosted 三主题中有合理值。

---

## 10. 动画与 transition 性能规则

禁止：

- `transition: all`
- 大面积 filter 动画
- 持续背景渐变 animation
- 常驻 `will-change`
- 为了“GPU 加速”无条件增加 `translateZ(0)` / `backface-visibility:hidden`

推荐只 transition 实际变化属性。

主按钮不得 idle infinite animation。

---

## 11. Terminal / SplitView

Terminal 是最高性能优先级内容面。

- xterm 可透明，但透明目标是 `.liquid-glass-content`。
- 非活动 terminal 保留实例/scrollback，但淡出后 `visibility:hidden`，停止无意义 paint/compositing。
- 不因切 Tab dispose/recreate xterm。
- active pane 比 inactive pane 更透。
- 不使用未经验证的 `content-visibility`。
- resize gesture 期间允许临时关闭 chrome backdrop sampling。

---

## 12. Sidebar

Sidebar 外壳是 chrome；session item 默认扁平：

- 默认：透明边框、无卡片阴影
- hover：轻 tint + border
- active：明确 tint + rim
- 不让所有会话都成为常驻独立玻璃卡

---

## 13. 控件

| 类 | 用途 |
|---|---|
| `.liquid-glass-button` | 次要按钮 |
| `.liquid-glass-ghost-button` | chrome 图标按钮 |
| `.liquid-primary-button` | 主 CTA |
| `.liquid-glass-input` | input |
| `.liquid-glass-select` | select |
| `.liquid-glass-textarea` | textarea |
| `.liquid-glass-toggle` | toggle |
| `.liquid-glass-dot` | 状态点 |
| `.glass-overlay` | 统一模态遮罩 |

`GlassPanel.surface` 使用 `chrome | content | accent` 明确材质语义。

---

## 14. 图标与 emoji

除 `src/components/FileManager/entryIcon.ts` 的文件类型 emoji 外，UI 不使用 emoji 充当控件图标。使用 `Icon` + `src/assets/icons/`。

---

## 15. 提交前检查

至少执行：

```bash
rg 'backdrop-filter' src/components src/renderers --glob '*.module.css'
rg 'transition:\s*all' src --glob '*.css'
rg 'filter:\s*blur|mix-blend-mode|will-change' src --glob '*.css' --glob '*.tsx'
rg 'liquid-glass-content|liquid-glass-accent|liquid-glass-float|liquid-glass' src --glob '*.tsx'
rg 'data-performance|data-motion|ambient-|content-surface' src
npm run build
```

视觉验证：

1. google-glow / obsidian / frosted
2. quality / balanced / compat
3. single pane / 4 panes
4. 3–5 秒内 Google Ambient motion 可感知
5. active pane 更透，inactive 更安静
6. modal 打开后仍能辨识主题
7. 多后台 xterm + 高速输出
8. focused / unfocused / hidden
9. resize / split drag

验收原则：

- **Google Glow 要活**
- **Obsidian 要深**
- **Frosted 要亮**
- **三个主题共享同一 Google 光**
- **Quality 更丰富、Balanced 更克制、Compat 真正静态**

## 实现源文件

- `src/styles/tokens.css`
- `src/styles/global.css`
- `src/context/ThemeContext.tsx`
- `src/components/Layout/GoogleGlowBackground.tsx`
- `src/components/Terminal/TerminalView.tsx`
- `src/components/Layout/SplitView.tsx`
