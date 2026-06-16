## Why

TauTerm v0.1 是一个串口助手雏形，离"全功能终端模拟器"还很远。用户反馈串口连接后按回车键无响应，界面简陋。需要进行深度架构重构，将 TauTerm 改造为能够替代 WindTerm/MobaXterm 的现代终端模拟器，同时保持串口+YModem 为核心功能，架构预留 SSH/Telnet 扩展能力。

## What Changes

- **修复串口 I/O 问题**：改用带缓冲通道和公平读写调度，解决回车键无响应（写饥饿）问题
- **多会话标签页架构**：引入 SessionManager 管理多个并发会话，每标签页独立 I/O 线程，支持标签页切换/关闭/重命名/拖拽排序
- **会话持久化**：SessionStore 保存/恢复会话配置到 JSON 文件，支持启动时自动恢复上次会话
- **Hub 式界面布局**：重新设计为"左侧会话列表 + 中央终端区（标签页）+ 顶部快速连接栏 + 底部文件传输面板"的专业布局
- **Liquid Glass 设计系统 v2**：升级玻璃拟态设计，Neon Dark 主题 + Framer Motion 交互动画（悬停光晕、呼吸灯、涟漪、扫光、拖拽阻尼）
- **终端搜索**：Ctrl+F 打开终端内搜索栏，支持向上/向下搜索和匹配高亮
- **命令面板**：Ctrl+Shift+P 打开命令面板，支持模糊搜索所有操作
- **主题引擎**：CSS 变量动态注入，支持多套主题即时切换，默认 Neon Dark
- **文件传输增强**：拖拽文件到窗口自动弹出 Dropzone，YModem 传输带流光扫光进度条
- **快捷键系统**：统一快捷键管理，可配置的键位绑定

## Capabilities

### New Capabilities

- `session-manager`: 多会话生命周期管理 — 创建、关闭、切换、重命名标签页，每会话独立 I/O 线程，缓冲通道通信
- `hub-layout`: Hub 式界面布局 — 左侧会话列表侧边栏 + 中央终端标签页区 + 顶部快速连接栏 + 底部文件传输面板 + 状态栏 + 可拖拽分割线
- `liquid-glass-theme`: Liquid Glass 设计系统 v2 — Neon Dark 主题、CSS 变量体系、Framer Motion 交互动画（hover 光晕、呼吸灯、涟漪、扫光、拖拽阻尼、Dropzone）
- `term-search`: 终端内容搜索 — Ctrl+F 搜索栏，正则/大小写匹配，上下导航，高亮显示
- `command-palette`: 命令面板 — Ctrl+Shift+P 打开，模糊搜索所有可用命令和快捷键
- `session-persistence`: 会话持久化 — 保存/恢复会话配置到 JSON，启动自动恢复
- `shortcut-system`: 快捷键系统 — 统一键位注册、冲突检测、可配置绑定
- `serial-io-fix`: 串口 I/O 修复 — 缓冲通道替换无缓冲通道，公平读写调度，消除写饥饿

### Modified Capabilities

- `serial-terminal`: 串口终端核心 — I/O 通道从无缓冲改为缓冲(32)，读写调度从"读优先"改为公平轮询 **BREAKING**（变更 internal API）
- `file-transfer`: 文件传输面板 — 新增 Dropzone 拖拽上传、扫光动画、传输进度优化
- `liquid-glass-ui`: 升级为 Liquid Glass v2 — 全部组件翻新，Neon Dark 主题替代旧玻璃效果 **BREAKING**（视觉设计变更）
- `i18n`: 国际化 — 新增会话管理、搜索、命令面板、快捷键相关翻译

## Impact

- **Rust 后端**：`src-tauri/src/session/` 重构为 `SessionManager` + `SessionHandle`，废除单一 `SerialSession` 持有
- **React 前端**：全部组件重写，引入 Framer Motion，新增 ~10 个组件
- **状态管理**：从散落 `useState` 重构为 `React Context + useReducer`
- **CSS**：`tokens.css` 扩展 3x，新增 Neon Dark 主题变量，移除部分旧 Glass 样式
- **依赖新增**：`framer-motion`（前端动画库）
- **无 API 破坏**：Tauri 命令接口保持兼容，增加新命令
