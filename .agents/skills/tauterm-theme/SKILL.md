---
name: tauterm-theme
description: "Single source of truth for TauTerm Liquid Glass UI, Gemini/Google ambient spectrum, clear-glass physics, theme tint veils, structural panels, control states, split/session presentation, motion, and rendering-performance rules."
license: MIT
metadata:
  author: tauterm
  version: "8.2"
---

# TauTerm Liquid Glass v8.2 — 唯一主题规范源

> **SSOT**：TauTerm 的主题、材质、Gemini 色谱、Liquid Glass Physics、Theme Veil、Structural Panel、SendBar、SplitView 视觉状态与渲染性能规则只在本文件维护。  
> `docs/` 不复制主题规则；`tauterm-theme-review` 只维护审查流程。

## 1. 设计模型

视觉系统由四个正交层组成：

1. **Gemini Ambient**：提供低频颜色与流动。
2. **Clear Liquid Glass Physics**：所有主题共享的透明玻璃本体。
3. **Theme Veil**：Google Glow / Obsidian / Frosted 只在同一玻璃上覆盖透明 / 黑 / 白薄膜。
4. **Performance Mode**：只改变采样和装饰动画成本，不改变材质身份。

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

- 三主题中 **veil 最弱**，且 Ambient opacity 可以略高于其它暗色主题以强化透亮感。
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

使用两个 oversized Field：

- Field A：Red（左上） + Green（右下）
- Field B：Blue（右上） + Yellow（左下）

只动画 transform：translate + 轻 rotation + 轻 scale。

- Balanced：约 20s / 24s
- Quality：约 14s / 17s
- 正常观察 3–5 秒必须能感知流动
- Compat：保留完整四色但静态
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

### 材质

Terminal xterm 本体透明到 `.liquid-glass-content`。

Content 与 Structural Panel 共用 clear physics，但：

- Content veil 更实；
- specular 更弱；
- outer shadow 更少；
- 重点是文字稳定和长时间阅读。

### 单 Pane

只有一个 Pane 时：

- 不显示 selected top line；
- 不显示 selected frame；
- Terminal 不套 active/inactive material；
- Content 使用 neutral `.liquid-glass-content`。

### 多 Pane

只有 `paneCount > 1` 时：

- selected pane 可使用 active veil；
- 其它 pane 可使用 inactive veil；
- Pane Header 使用 `--theme-content-header-veil` + 同一 content clear base；
- Pane 圆角必须遵循 **outer-corner only**：Workspace 四个真实外角可圆，所有内部 Split 交点必须为直角；
- Pane Frame、Header、Content/Terminal 必须使用同一套 corner geometry，禁止各自写固定四角圆角。

### Disconnected

Network Debug 与其它会话共用 SplitView 的 `PaneEmptyState`。

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

Quality：
- 2 dynamic Ambient Fields
- Small Chrome / Float 较高 blur sampling

Balanced：
- 2 dynamic Ambient Fields
- 默认 20s / 24s motion
- Small Chrome / Float 8–10px 级 sampling

Compat：
- 2 static Ambient Fields
- backdrop blur = 0
- Structural / Content 保持正常 clear base + theme veil，不额外变厚
- **依赖 backdrop 的 Small Chrome / Float 必须切换到更实的 `--theme-*-veil-compat` fallback**，避免底层文字和边框无模糊地直接穿透
- Compat 可以比 Balanced 更“稳”和更实，但不能比 Balanced 感知上更透明，也不能退回死板的实心 opaque surface

---

## 14. 提交前审计

```bash
rg 'backdrop-filter' src/components src/renderers --glob '*.module.css'
rg 'transition:\s*all' src --glob '*.css'
rg 'filter:\s*blur|mix-blend-mode|will-change' src --glob '*.css' --glob '*.tsx'
rg -U ':disabled[^\{]*\{[^\}]*opacity\s*:\s*0\.' src --glob '*.css'
rg 'liquid-glass-panel|liquid-glass-content|liquid-control-surface|liquid-glass-float|liquid-glass' src --glob '*.tsx'
rg 'theme-(chrome|panel|content|card|float|control).*veil|theme-(chrome|float)-veil-compat|liquid-clear-|liquid-specular-' src/styles
rg 'border-radius.*pane|pane(Frame|Header|Content)Radius|dockedBorderRadii' src/components/Layout src/components/Terminal
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
- TargetBar visible / hidden
- Ambient 连续 ≥10s
- disabled controls 位于四色最亮区域

验收句：

- **发送栏四个模式仍在左侧竖排**
- **左栏、右栏、SendBar、TargetBar 是同一种 Structural Glass**
- **Terminal 是同一种玻璃的克制 Content 版本**
- **炫彩流光明显最透亮，Ambient 穿透最清楚**
- **黑曜石 = clear glass + 更厚 black veil，必须明显比炫彩流光更黑**
- **白霜 = clear glass + white veil**
- **三个主题都有清澈、柔亮、边缘高光的液态玻璃质感**
- **单 Pane 没有 active/selected 材质差异**
- **多 Pane 只有 Workspace 外角圆，内部交点直**
- **Compat 仍然是玻璃，但 Small Chrome/Float 的可读性不能低于 Balanced，也不能出现“兼容档最透明”**

## 实现源文件

- `src/styles/tokens.css`
- `src/styles/global.css`
- `src/context/ThemeContext.tsx`
- `src/App.tsx`
- `src/components/Layout/GoogleGlowBackground.tsx`
- `src/components/Layout/SessionSidebar.tsx`
- `src/components/RightSidebar/RightSidebar.tsx`
- `src/components/Layout/SplitView.tsx`
- `src/components/Layout/SplitView.module.css`
- `src/components/Terminal/Terminal.tsx`
- `src/components/Terminal/TerminalView.tsx`
- `src/components/SendBar/SendBar.tsx`
- `src/components/SendBar/SendBar.module.css`
- `src/components/SendBar/TargetBar.tsx`
