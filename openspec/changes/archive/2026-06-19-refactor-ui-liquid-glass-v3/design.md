## Context

TauTerm 当前使用 CSS Modules + CSS 自定义属性（设计令牌）体系实现 Liquid Glass v2 主题。3 套主题（neon-dark / ocean / sunset）通过 `data-theme` 属性切换，令牌定义在 `src/styles/tokens.css`，组件通过 `var(--token)` 引用。Framer Motion 已集成用于交互动画。

本次 v3 升级的核心技术挑战是：在不引入新 CSS 框架的前提下，将一套依赖 Tailwind 原型验证的设计体系无缝移植到现有 CSS Modules 架构中。

### 约束

- 不引入 Tailwind CSS 或其他 CSS 框架
- 保持 CSS Modules + 自定义属性模式
- 暂不修改 Tauri 窗口配置（保持 `decorations: true`）
- XTerm 终端背景必须保持透明以透出流光效果
- 需考虑 `blur(120-140px)` 和 SVG 噪点在 WebView 中的 GPU 性能

## Goals / Non-Goals

**Goals:**

1. 用 google-glow / obsidian / frosted 三套主题替换现有主题，所有 UI 元素通过令牌自动适配
2. 新增 CSS `@keyframes` 动画（morph / flow1~4）和全局光球样式类
3. 创建 `GoogleGlowBackground` 组件渲染 4 个动态光球
4. 将 SVG `feTurbulence` 噪点纹理注入玻璃面板
5. 实现不对称边框高光（top/left 亮于 bottom/right）
6. 升级 GlassPanel / GlassButton / GlassInput 的视觉效果
7. 调整 Toolbar / Sidebar / Terminal / SendBar 区块深度
8. 更新 ThemeContext 和 AppearanceSettings 以支持新主题

**Non-Goals:**

- Tauri 无边框窗口（后续迭代）
- 主题热插拔/自定义主题系统
- 响应式移动端适配（桌面端专属）
- 终端配色方案同步切换（XTerm 主题配置独立于 UI 主题）

## Decisions

### 决策 1：沿用 CSS 自定义属性令牌体系，不引入 Tailwind

**选择**：扩展 `tokens.css`，为 3 套新主题定义完整的 CSS 变量集合。

**理由**：
- 现有组件已通过 `var(--token)` 引用令牌，改值不改结构
- 零新依赖，零构建配置变更
- 3 套主题的本质差异是同一组属性的不同取值，天然适合自定义属性
- 原型中的 Tailwind 工具类（`bg-[#080808]`, `backdrop-blur-md`, `rounded-[1.5rem]` 等）可 1:1 映射到令牌或固定 CSS 值

**替代方案已排除**：
- **Tailwind CSS**：引入新的构建依赖和样式范式，与现有 CSS Modules 混用增加复杂度
- **UnoCSS**：虽比 Tailwind 轻量，但仍是新依赖，且同样造成两套范式混用
- **CSS-in-JS**：与 CSS Modules 方向相反，无必要

### 决策 2：三级令牌模型

将设计令牌分为三个层级，清晰隔离变与不变：

```
Level 1 — 共享常量（不随主题变化）
  --font-ui, --font-mono      字体
  --radius-sm/md/lg/xl/full   圆角
  --spacing-xs~2xl            间距
  --transition-fast/normal    过渡
  --z-sidebar/panel/overlay   层级
  
Level 2 — 主题令牌（每个主题定义自己的值）
  --bg-base                    页面底色
  --bg-orb-opacity             光球透明度
  --bg-orb-blur                光球模糊半径
  --bg-orb-blend               光球混合模式
  --glass-fill                 玻璃填充渐变
  --glass-noise-opacity        噪点透明度
  --glass-noise-frequency      噪点频率
  --glass-blur                 玻璃模糊度
  --glass-blur-saturate        玻璃饱和度
  --glass-border-top           顶部边框
  --glass-border-left          左侧边框
  --glass-border-default       默认边框
  --glass-shadow-outer         外阴影
  --glass-shadow-inner         内高光
  --glass-button-bg            按钮背景
  --glass-button-border        按钮边框
  --glass-button-hover-bg      按钮悬停背景
  --glass-button-hover-border  按钮悬停边框
  --glass-input-bg             输入框背景
  --glass-input-border         输入框边框
  --glass-input-shadow-inner   输入框内阴影
  --glass-input-focus-border   输入框聚焦边框
  --glass-input-focus-glow     输入框聚焦光晕
  --block-toolbar-bg           Toolbar 背景
  --block-sidebar-bg           Sidebar 背景
  --block-terminal-bg          Terminal 背景
  --block-sendbar-bg           SendBar 背景
  --text-primary               主文字色
  --text-secondary             辅助文字色
  --text-muted                 弱化文字色
  --accent-primary             强调色
  --accent-secondary           辅助强调色
  --accent-gradient            强调渐变
  --accent-glow                强调发光
  --color-success/error/warning/info  状态色
  
Level 3 — 组件级（组件内 calc/组合，不直接定义在 tokens.css）
  组件 CSS Module 中使用 var() 组合或 calc() 派生
```

### 决策 3：GoogleGlowBackground 作为独立组件

**选择**：创建 `src/components/Layout/GoogleGlowBackground.tsx` 作为独立组件，在 `App.tsx` 中挂载。

**理由**：
- 单一职责：仅负责渲染 4 个光球 div 和注入 `<style>` 标签
- 易于测试、替换和条件渲染（未来可加开关）
- 不污染全局 CSS 文件
- 光球颜色固定为 Google 四色（所有主题共享），仅透明度/模糊/混合模式随主题变化

### 决策 4：SVG 噪点纹理通过 CSS 背景层注入

**选择**：在 `.liquid-glass` 类的 `background-image` 中使用 Data URI 内联 SVG `feTurbulence` 滤镜，配合渐变背景形成多层背景。

**理由**：
- 纯 CSS 方案，无需额外资源文件
- `feTurbulence` 的 `baseFrequency` 和 `opacity` 可通过令牌为每套主题微调：Google Glow 用 0.8/0.04，Obsidian 用 0.85/0.05，Frosted 用 0.9/0.03
- 噪点叠加在渐变之上，产生磨砂质感

**风险**：SVG Data URI 在 CSP 限制下可能被阻止。当前 CSP 允许 `style-src 'self' 'unsafe-inline'`，Data URI 的样式不需要额外 CSP 配置，但需验证。

### 决策 5：不对称边框通过独立 border 属性实现

**选择**：使用 `border` + `border-top` + `border-left` 覆盖实现不对称高光，而非 `box-shadow` 模拟。

```
border: 1px solid var(--glass-border-default);   /* 默认四边 */
border-top: 1px solid var(--glass-border-top);    /* 顶部更亮 */
border-left: 1px solid var(--glass-border-left);  /* 左侧次亮 */
```

**理由**：原生 CSS 方式，无性能开销。比 `box-shadow` 模拟边框更精确。

### 决策 6：组件样式升级策略——增量增强

**选择**：不重写组件，而是在现有 CSS Module 中增量增强。

**具体策略**：
- `GlassPanel.module.css`：增加 `background-image`（噪点 + 渐变），不对称边框属性，增强阴影
- `GlassButton.module.css`：增加 `translateY(-2px)` 悬停位移，增强阴影和边框变化
- `GlassInput.module.css`：增加更深的 `inset box-shadow`，聚焦蓝色光晕，调整 placeholder 颜色
- 布局组件的 CSS Module：调整各区块 `background` 透明度以匹配新主题深度

### 决策 7：性能策略

| 措施 | 说明 |
|------|------|
| `will-change: transform, border-radius` | 仅应用于 4 个光球，提示 GPU 预合成 |
| `pointer-events: none` | 光球容器不参与事件处理 |
| `overflow: hidden` | 光球容器裁剪溢出，减少重绘区域 |
| `mix-blend-mode: screen/multiply` | 依赖 GPU 合成器，不触发 layout |
| 噪点纹理为静态 SVG | 不产生持续动画开销，仅首次绘制成本 |
| 尊重 `prefers-reduced-motion` | 未来迭代中加入，当前版本先硬编码动画 |

## Risks / Trade-offs

| 风险 | 缓解措施 |
|------|---------|
| `blur(120-140px)` 在低端 GPU / 嵌入式 WebView 上可能掉帧 | 光球已设置 `will-change`；可在后续版本中检测设备能力并降级模糊半径 |
| SVG 噪点纹理可能增加首次渲染时间 | 噪点纹理极简（200x200 viewBox），实际渲染成本低 |
| 3 套主题的令牌差异大，维护成本增加 | 令牌按 Level 1/2/3 分层，共享常量不重复定义 |
| Frosted 浅色主题下终端可读性可能下降 | 终端区保留 `bg-white/30` 半透背景 + 深色文字 `text-slate-700` |
| 现有主题被 **BREAKING** 删除，用户设置中的主题偏好将失效 | localStorage 中的 `tauterm-theme` 值将被重置为 `google-glow` 默认值 |

## Open Questions

- 需要在哪些具体设备/WebView 版本上测试 GPU 性能？（Windows WebView2 为主要目标）
- CSP 是否需要显式允许 `data:` 来源的样式？（当前 `'unsafe-inline'` 应已覆盖）
- 后续是否考虑让用户自定义光球颜色？（当前固定 Google 四色）
