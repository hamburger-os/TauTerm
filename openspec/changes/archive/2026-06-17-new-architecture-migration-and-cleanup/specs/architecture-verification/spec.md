# architecture-verification

## Purpose

验证 TauTerm 前后端代码已完全切换至新微内核插件架构，确保不存在对已删除旧模块的过期引用。

## ADDED Requirements

### Requirement: Backend module declarations are fully migrated
后端 `lib.rs` 的模块声明 SHALL 仅引用新架构模块（`channel`、`kernel`、`plugins`、`security`、`transfer`），不得包含已删除的 `serial`（顶层）、`session`（顶层）模块声明。

#### Scenario: All mod declarations reference new modules
- **WHEN** 检查 `src-tauri/src/lib.rs` 中的 `mod` 声明
- **THEN** 声明列表 SHALL 仅包含 `channel`、`commands`、`kernel`、`plugins`、`security`、`transfer`
- **AND** 不存在 `mod serial;` 或 `mod session;` 声明（旧模块已完全移除）

### Requirement: Frontend imports reference new modules only
前端所有 TypeScript/TSX 文件的 import 语句 SHALL 仅引用新架构模块（`core/`、`renderers/`、`plugins/`），不得包含对已删除文件（`useSerialPort`、`useFileTransfer`、`SerialConfigSidebar`、`GlassSelect`）的引用。

#### Scenario: No stale imports from deleted files
- **WHEN** 对 `src/` 目录执行 `grep` 搜索已删除文件名
- **THEN** `useSerialPort`、`useFileTransfer`、`SerialConfigSidebar`、`GlassSelect` SHALL 不出现于任何 import 语句中
- **AND** TypeScript 编译 (`npm run build`) SHALL 成功，无模块解析错误

### Requirement: Plugin registry is the sole source of UI extension
前端 SHALL 通过 `core/plugin-registry` 的 `registerPlugin()` 和 `pluginRegistry.get()` 管理插件 UI 组件，不得存在硬编码的插件特定组件引用（如直接 import SerialConfigSidebar）。

#### Scenario: Toolbar gets items from plugin registry
- **WHEN** Toolbar 组件渲染
- **THEN** 插件按钮列表 SHALL 通过 `pluginRegistry.get(activeTab.pluginId)?.toolbarItems` 获取
- **AND** 不存在硬编码的插件特定工具栏按钮

#### Scenario: TabContentDispatcher uses registry for content types
- **WHEN** TabContentDispatcher 选择渲染器
- **THEN** 内容类型 SHALL 通过 `pluginRegistry.get(activeTab.pluginId)?.manifest.content_type` 确定
- **AND** switch-case 按 `content_type` 值分发到通用渲染器（TerminalRenderer、FileBrowserRenderer 等）

### Requirement: Rust plugin host manages plugin lifecycle
后端 SHALL 通过 `kernel::plugin_host::PluginHost` 管理插件注册和查询，不得在 commands.rs 中硬编码串口特定逻辑路径。

#### Scenario: Plugin host is initialized at startup
- **WHEN** 应用启动 (`lib.rs` 的 `run()` 函数)
- **THEN** `PluginHost::new()` SHALL 被调用并注册至少一个内建插件（Serial）
- **AND** `AppState` SHALL 包含 `plugin_host: Mutex<PluginHost>`

#### Scenario: Connection types are enumerated via plugins
- **WHEN** 前端调用 `get_connection_types` 命令
- **THEN** 命令 SHALL 通过 `plugin_host` 查询已注册的插件来生成连接类型列表
- **AND** 不存在硬编码的 `"serial"` 连接类型列表
