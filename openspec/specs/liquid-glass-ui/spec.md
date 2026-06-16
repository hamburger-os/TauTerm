# liquid-glass-ui

## Purpose

定义 Liquid Glass 设计系统要求，包括玻璃拟态视觉风格（v2 升级）、暗色主题、排版、响应式布局、Framer Motion 动画集成和交互动效。

## Requirements

### Requirement: Liquid Glass 设计语言
界面必须实现升级版玻璃拟态设计系统（v2），特征为 Neon Dark 主题、更深的背景层次、Framer Motion 驱动的交互动画。

#### Scenario: 玻璃面板渲染
- **WHEN** 应用窗口显示
- **THEN** 所有面板表面必须以 `backdrop-filter: blur(16px)`、半透明深色背景（`rgba(0, 212, 170, 0.04)`）和低透明度青色（`rgba(0, 212, 170, 0.12)`）的 1px 微细边框渲染，背景为深邃暗色（`#060610`）

#### Scenario: 深度和层次感
- **WHEN** 不同层级的玻璃面板重叠
- **THEN** 重叠区域必须显示增强的模糊和变暗效果，通过各层营造视觉深度感，Z-index 清晰分层

#### Scenario: 悬停和激活状态
- **WHEN** 用户悬停或交互 UI 元素（按钮、输入框、选择器）
- **THEN** 元素必须显示背景透明度、边框亮度的平滑过渡，以及微妙的发光效果

### Requirement: 暗色主题基础
界面必须使用 Neon Dark 主题作为默认基础，深色星空背景配合青色/蓝色霓虹强调色。

#### Scenario: 背景渐变
- **WHEN** 应用渲染
- **THEN** 根背景必须显示从深空黑（`#060610`）到深蓝黑（`#0a0a1a`）的平滑渐变

#### Scenario: 文字对比度
- **WHEN** 文字显示在玻璃面板上
- **THEN** 主文字必须使用 `#e0e0ff`，辅助文字必须使用 `#8888aa`，强调文字使用 `#00d4aa` 带 text-shadow 发光

### Requirement: 强调色和渐变
系统必须使用青绿到天蓝的霓虹色作为主要强调色，带发光效果。

#### Scenario: 主按钮使用强调渐变
- **WHEN** 主操作按钮（连接、发送、接收）在默认状态下渲染
- **THEN** 必须以青绿到天蓝渐变（`#00d4aa → #00a3ff`）作为其背景色，并带有 `box-shadow` 发光效果

#### Scenario: 焦点和选中指示
- **WHEN** UI 元素获得焦点或被选中
- **THEN** 必须显示 3px 宽的青色霓虹指示线（左侧或底部），并带有发光阴影

#### Scenario: 进度指示
- **WHEN** 显示进度条（文件传输、连接状态）
- **THEN** 已填充部分必须使用强调渐变

### Requirement: 终端排版
终端视口必须使用优化的等宽字体以提高可读性，UI 装饰使用清爽的无衬线字体。两种字体均需支持中文字符渲染。

#### Scenario: 终端字体
- **WHEN** 终端渲染文字
- **THEN** 必须使用 JetBrains Mono（回退到 Cascadia Code、Fira Code 或系统等宽字体）以舒适的阅读大小显示

#### Scenario: UI 字体
- **WHEN** UI 装饰元素（标签、按钮、菜单）渲染文字
- **THEN** 必须使用 Inter（回退到系统无衬线字体），具有适当的字重和大小层次结构。中文字符必须能正常回退到系统默认中文字体

### Requirement: 可调整面板的响应式布局
应用布局必须适应窗口大小变化，并允许用户调整侧边栏和文件传输面板的大小。

#### Scenario: 窗口大小调整
- **WHEN** 用户调整应用窗口大小
- **THEN** 终端视口必须填满可用空间，侧边栏必须在其最小/最大约束内保持宽度，所有玻璃面板必须以新尺寸正确重新渲染模糊效果

#### Scenario: 可调整的侧边栏
- **WHEN** 用户拖动侧边栏与终端之间的分隔线
- **THEN** 侧边栏宽度必须在 200px（最小）和 400px（最大）之间平滑调整

#### Scenario: 可调整的文件传输面板
- **WHEN** 用户拖动文件传输面板与终端之间的分隔线
- **THEN** 面板高度必须在 150px（最小）和窗口高度的 50%（最大）之间平滑调整

### Requirement: 流畅动画和过渡
界面必须使用 Framer Motion 和 CSS transition 处理所有状态变化和交互反馈。

#### Scenario: 面板滑动动画
- **WHEN** 互动面板打开或关闭
- **THEN** 必须以 Framer Motion spring 动画过渡，无布局跳动或闪烁

#### Scenario: 悬停微交互
- **WHEN** 用户悬停在交互元素上
- **THEN** 必须在 0.3s 内发生背景亮度变化、边框发光、轻微缩放（`scale(1.02)`）效果

#### Scenario: 连接状态过渡
- **WHEN** 连接状态变化
- **THEN** 状态指示器必须播放呼吸灯动画（connecting: 1.5s pulse），成功后触发一次涟漪动画（scale + opacity 渐变消失）

### Requirement: Framer Motion 动画集成
系统必须集成 Framer Motion 库驱动所有 UI 交互动画。

#### Scenario: 组件进出动画
- **WHEN** 组件挂载或卸载（如标签页切换、面板开关）
- **THEN** 必须使用 AnimatePresence 播放进场和退场动画

#### Scenario: 拖拽排序
- **WHEN** 用户拖拽标签页重新排序
- **THEN** 必须使用 Framer Motion `drag` 和 `Reorder` 组件实现平滑的拖拽重排

#### Scenario: 手势交互
- **WHEN** 用户与 UI 元素进行复杂手势交互（如拖拽调整面板大小）
- **THEN** 必须包含阻尼感和视觉反馈（如 resize handle 发光线的渐变出现/消失）
