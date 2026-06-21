## Why

TauTerm 当前的 Liquid Glass v2 主题系统（neon-dark / ocean / sunset）仅实现了基础的玻璃拟态效果——简单的半透明背景 + 模糊滤镜。随着终端应用对视觉沉浸感的要求提升，用户期望一套更具深度、动态感和材质真实感的 UI。此次 v3 升级引入**动态流光背景**、**SVG 噪点磨砂纹理**、**不对称高光边框**和**黑曜石暗色 / 白霜浅色双模主题**，使 TauTerm 在视觉上达到现代桌面应用的顶级水准。

## What Changes

- **主题系统替换**：删除现有 neon-dark / ocean / sunset 三套主题，替换为 google-glow（默认，Google 四色动态光球 + 液态玻璃）、obsidian（黑曜石暗色，近纯黑底 + 霓虹光球 + 磨砂黑玻璃）、frosted（白霜浅色，灰白底 + 漫反射光球 + 清透白霜玻璃）
- **动态光球背景**：新建 `GoogleGlowBackground` 组件，4 个 Google 四色光球应用 `morph` 不规则形变 + `flow` 盘旋运动动画
- **液态玻璃 v3 材质**：引入 SVG `feTurbulence` 噪点纹理注入玻璃层实现磨砂质感；不对称边框高光（top/left 更亮）；增强的模糊度（25-35px）和多层阴影
- **组件玻璃效果升级**：GlassPanel / GlassButton / GlassInput 组件支持噪点纹理、不对称边框和更强的交互反馈
- **布局区块深度重定义**：Toolbar / Sidebar / Terminal / SendBar 的背景深度和模糊度按三套主题各自调整
- **主题上下文更新**：ThemeContext 支持新的 3 个主题 ID，默认主题改为 `google-glow`
- **外观设置面板更新**：AppearanceSettings 展示新的主题选项

## Capabilities

### New Capabilities

- `dynamic-orb-background`: 动态光球背景系统——4 个 Google 四色光球，应用 CSS `@keyframes morph` 不规则形变和 `flow` 盘旋运动轨迹，根据主题切换光球透明度、模糊半径和混合模式
- `liquid-glass-noise-texture`: SVG 噪点纹理注入——通过内联 SVG `feTurbulence` 滤镜在玻璃面板中注入磨砂噪点，增强材质真实感

### Modified Capabilities

- `liquid-glass-theme`: **BREAKING** 删除 neon-dark / ocean / sunset 三套主题，替换为 google-glow / obsidian / frosted；CSS 令牌体系扩展以支持光球参数、噪点纹理、不对称边框等新属性
- `liquid-glass-ui`: **BREAKING** 玻璃面板视觉规范从 v2 升级到 v3——模糊度从 16px 提高到 25-35px，边框从对称改为不对称高光，新增 SVG 噪点纹理要求，阴影从单层改为多层

## Impact

- **样式系统**: `src/styles/tokens.css`（重写）, `src/styles/global.css`（新增动画和全局类）
- **组件**: `GlassPanel.tsx` / `GlassButton.tsx` / `GlassInput.tsx` 及其 CSS Module（增强玻璃效果）
- **布局**: `App.tsx` / `App.css`, `AppShell.tsx`, `Toolbar.tsx` / `Toolbar.module.css`, `SessionSidebar.tsx` / `SessionSidebar.module.css`, `TerminalView.tsx` / `Terminal.module.css`, `SendBar.tsx` / `SendBar.module.css`
- **状态/配置**: `ThemeContext.tsx`（主题 ID 枚举更新）, `AppearanceSettings.tsx`（主题选项更新）
- **新建文件**: `src/components/Layout/GoogleGlowBackground.tsx`
- **依赖**: 无需新增 npm 依赖。Framer Motion 已安装，继续使用。不引入 Tailwind CSS。
- **Tauri 配置**: 暂不修改（保持 `decorations: true`），后续迭代再考虑无边框窗口
