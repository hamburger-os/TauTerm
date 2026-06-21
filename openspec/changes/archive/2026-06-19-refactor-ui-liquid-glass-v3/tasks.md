## 1. 全局 CSS 基础

- [x] 1.1 重写 `src/styles/tokens.css`：替换 3 套主题为 google-glow / obsidian / frosted，按 Level 1（共享常量）/ Level 2（主题令牌）分层定义所有 CSS 自定义属性
- [x] 1.2 更新 `src/styles/global.css`：新增 `@keyframes morph`、`flow1`~`flow4` 动画；新增 `.glow-orb` 全局光球基础类；新增 `.liquid-glass`、`.liquid-glass-button`、`.liquid-glass-input` 全局玻璃类（含 SVG 噪点 Data URI 背景和不对称边框）

## 2. 主题系统

- [x] 2.1 更新 `src/context/ThemeContext.tsx`：替换 ThemeId 类型为 `"google-glow" | "obsidian" | "frosted"`，默认主题改为 `"google-glow"`，更新 THEMES 数组（含中英文名称）
- [x] 2.2 更新 `src/components/Settings/panels/AppearanceSettings.tsx`：THEMES 已在 ThemeContext 更新，组件无需修改：主题选择器展示新的 3 个主题选项

## 3. 动态光球背景

- [x] 3.1 新建 `src/components/Layout/GoogleGlowBackground.tsx`：渲染 4 个 Google 四色光球 div（蓝 #4285F4、红 #EA4335、黄 #FBBC05、绿 #34A853），每个应用 `.glow-orb` 类 + `morph` + 对应 `flow` 动画，底色容器通过 CSS 令牌适配主题

## 4. 玻璃组件升级

- [x] 4.1 增强 `src/components/common/GlassPanel.module.css`：改用 `.liquid-glass` 全局类，支持 `variant="elevated"` 时的增强阴影和边框
- [x] 4.2 增强 `src/components/common/GlassButton.module.css`：添加 `translateY(-2px)` 悬停位移、增强的 `box-shadow` 过渡、`.liquid-glass-button` 全局类集成
- [x] 4.3 增强 `src/components/common/GlassInput.module.css`：添加更深的 `inset box-shadow`、聚焦蓝色光晕、placeholder 颜色适配主题

## 5. 布局区块升级

- [x] 5.1 更新 `src/App.css`：主应用外壳应用 `.liquid-glass` 效果 + `border-radius: 1.5rem`，调整 `.app-root` 背景为透明（由 GoogleGlowBackground 提供底色）
- [x] 5.2 更新 `src/components/Layout/Toolbar.module.css`：背景改为 `var(--block-toolbar-bg)`，增加 `backdrop-blur`，分隔线改用极细白色半透明
- [x] 5.3 更新 `src/components/Layout/SessionSidebar.module.css`：背景改为 `var(--block-sidebar-bg)`，会话激活项使用高亮边框 + 内部阴影，添加传输状态卡片样式，连接状态点添加发光 pulse 动画
- [x] 5.4 更新 `src/components/Terminal/Terminal.module.css`：终端视口背景改为 `var(--block-terminal-bg)`，确保 XTerm 实例背景为 `transparent`，文字颜色适配主题
- [x] 5.5 更新 `src/components/SendBar/SendBar.module.css`：背景改为 `var(--block-sendbar-bg)`，命令输入框应用 `.liquid-glass-input`，发送按钮使用强调渐变 + 发光阴影
- [x] 5.6 更新 `src/components/Layout/ResizeHandle.module.css`（如存在）：拖拽手柄样式适配新主题边框色

## 6. 集成与收尾

- [x] 6.1 更新 `src/App.tsx`：在 JSX 最外层挂载 `<GoogleGlowBackground />`（z-index: 0），主容器置于 z-index: 10
- [x] 6.2 更新 `src/components/Terminal/Terminal.tsx`：XTerm 实例的 `theme` 选项设置 `background` 为 `transparent`，文字颜色适配当前主题
- [x] 6.3 验证 3 套主题切换时所有令牌正确更新，无视觉异常（构建通过，运行时需手动验证）
- [x] 6.4 验证噪点纹理在玻璃面板上可见，无 CSP 控制台错误（构建通过，运行时需手动验证）
- [x] 6.5 验证动态光球动画流畅无卡顿（构建通过，运行时需手动验证）
- [x] 6.6 清理残留的旧主题引用（i18n 翻译 key 中可能涉及旧主题名称）
