## 1. 内核模块基础架构

- [x] 1.1 创建 `src-tauri/src/kernel/` 模块目录结构，建立 `mod.rs` 及各子模块入口文件
- [x] 1.2 实现 `ConfigStore` 模块——类型安全的 key-value 存储，支持 `get<T>()`、`set()`、`watch()`、`delete()`，JSON Schema 校验，命名空间隔离
- [x] 1.3 实现 `IpcBridge` 模块——插件命令注册表 `register_command()`、类型事件总线 `emit()`/`subscribe()`、Stream 通道
- [x] 1.4 实现 `TabHost` 模块——标签页 CRUD（`create_tab`、`close_tab`、`activate_tab`、`reorder_tabs`），会话生命周期事件（`tab-created`、`tab-closed`、`tab-activated`）
- [x] 1.5 实现 `ShortcutEngine` 模块——全局/插件作用域快捷键注册、冲突检测、作用域分发（活跃插件优先）
- [x] 1.6 实现 `ThemeEngine` 模块——CSS 自定义属性生成、运行时主题切换、插件自定义 token 注入
- [x] 1.7 实现 `I18nEngine` 模块——命名空间隔离翻译、插件注册翻译资源、运行时语言切换
- [x] 1.8 实现 `WindowManager` 模块——窗口创建/关闭、布局持久化、分屏状态管理（基础骨架，多窗口和分屏在后续版本完善）
- [x] 1.9 重构 `AppState`——从 `Mutex<SessionManager>` 改为持有各内核模块实例

## 2. I/O 通道抽象层

- [x] 2.1 定义 `Channel` trait——`read()`、`write()`、`flush()`、`set_timeout()`、`is_connected()`，确保 object-safe
- [x] 2.2 实现 `SerialChannel`——包装 `Box<dyn SerialPort>` 实现 `Channel` trait
- [ ] 2.3 重构 `spawn_io_loop` 以使用 `dyn Channel`——当前使用 `Box<dyn SerialPort>`，需改为协议无关的 `Channel` trait（保持现有功能可用，重构在后续会话完成）
- [x] 2.4 定义 `IoStrategy` 枚举——`Sync` 和 `Async` 变体，异步变体使用 `tokio::spawn`
- [ ] 2.5 实现异步 I/O 循环——基于 tokio 的 `spawn_async_io_loop`，使用 `tokio::sync::mpsc` 作为写入命令通道
- [x] 2.6 添加 `tokio` 依赖到 `Cargo.toml`，配置 `rt`、`sync`、`time`、`macros`、`net` features
- [x] 2.7 定义 `SessionError` 结构化错误枚举——`ConnectionFailed`、`PluginNotFound`、`CapabilityDenied`、`Timeout`、`IoError`、`AuthFailed` 等变体

## 3. 插件系统

- [x] 3.1 实现 `PluginHost` 模块——插件注册表、生命周期管理（discover → load → init → ready → stop → unload）
- [x] 3.2 定义 `PluginManifest` 结构体——`id`、`name`、`version`、`category`、`content_type`、`capabilities`、`config_schema` 字段
- [x] 3.3 定义 `ProtocolAdapter` trait——`connect()`、`disconnect()`、`discover_endpoints()`、`content_type()`、`transfer_protocols()`、`io_strategy()`
- [x] 3.4 实现能力声明校验——插件仅能使用其 `capabilities` 中声明的系统能力
- [x] 3.5 实现前端 `registerPlugin()` API——插件注册入口，接收 manifest、connectForm、toolbarItems、contextMenuItems、bottomPanels、statusBarItems、locales
- [x] 3.6 创建 `@tauterm/core` 前端内核包（`src/core/`）——导出 `registerPlugin`、`usePluginRegistry`、`useTabHost`、`useConfigStore` 等 hooks
- [x] 3.7 实现前端插件注册表 `PluginRegistry`——存储已注册插件的 manifest 和 UI 组件映射

## 4. Serial 插件化（重构现有代码）

- [x] 4.1 创建 `src-tauri/src/plugins/serial/` 模块，移动 `session/serial.rs` 和 `serial/config.rs` 到插件目录
- [x] 4.2 为 Serial 实现 `ProtocolAdapter` trait——`connect()` 打开串口返回 `SerialChannel`，`discover_endpoints()` 枚举串口，`content_type()` 返回 `"terminal"`
- [x] 4.3 创建 Serial 插件前端注册（`src/plugins/serial/`）——工具栏按钮、i18n 资源
- [x] 4.4 重构 `connect_session` Tauri 命令——接收 `plugin_id` 参数，通过 Plugin Host 查找 adapter
- [x] 4.5 重构 `disconnect_session`——保持现有 SessionManager 路径，支持 plugin_id
- [x] 4.6 重构 `enumerate_endpoints`——改为接收 `plugin_id` 参数
- [x] 4.7 重构 `get_connection_types`——从 Plugin Host 注册表动态获取
- [x] 4.8 确保所有现有功能（串口连接、HEX 模式、会话持久化、I/O 统计）在重构后正常工作

## 5. 传输子系统重构

- [x] 5.1 创建 `TransferManager` 模块——策略选择逻辑
- [x] 5.2 实现 Inline 传输策略——保持现有端口移交机制，I/O 循环使用 Channel HandoffPort
- [x] 5.3 定义 `TransferProtocolType` 枚举——YModem/XModem/ZModem/SFTP/SCP/FTP
- [ ] 5.4 重构 YModem 实现——适配新的 `Channel` trait（YModem 功能正常工作，完全迁移待后续）
- [x] 5.5 实现协议注册表框架——`TransferManager::select_strategy()`
- [x] 5.6 重构前端 `TransferContext`——COMMAND_MAP 支持多协议路由
- [x] 5.7 统一传输事件载荷——所有策略使用相同事件格式

## 6. 前端统一标签页渲染

- [x] 6.1-6.5 实现所有内容渲染器（Terminal/FileBrowser/StatsDashboard/Custom/TabContent）
- [x] 6.6 重构 `Toolbar`——插件槽位注入（left/center/right），活跃标签动态替换
- [ ] 6.7 重构 `BottomPanel`——支持插件注册的标签页（基础架构就绪，具体面板待后续）
- [x] 6.8 重构 `StatusBar`——聚合插件状态项 + I/O 吞吐量显示
- [x] 6.9 重构 `ConnectDialog`——从 Plugin Registry 动态生成协议卡片
- [ ] 6.10 重构 `AppShell`——使用新的内核 Context Provider（现有 AppShell 保持兼容）

## 7. 凭据存储 & 安全模型

- [ ] 7.1 添加 `keyring` 和 `aes-gcm` 依赖（内存后端已完成）
- [x] 7.2 实现 `CredentialStore`——内存后端完整实现
- [ ] 7.3-7.4 keyring/AES 降级后端待后续
- [x] 7.5 实现凭据类型枚举
- [x] 7.6 暴露 Tauri 命令——store/get/list/delete_credential
- [x] 7.7 实现日志脱敏
- [ ] 7.8 实现主机密钥验证

## 8. 新协议插件实现

- [ ] 8.1-8.10 所有新协议插件（SSH/Telnet/TCP Raw/TRDP/Shell/FTP/iPerf3）留待后续会话实现

## 9. 文档与 README

- [x] 9.1 编写全新 README.md
- [x] 9.2 创建 `docs/architecture/` 及四份架构文档
- [x] 9.3 编写插件开发指南
- [ ] 9.4 更新 `CONTRIBUTING.md`

## 10. 测试 & 验证

- [ ] 10.1 为 `Channel` trait 编写单元测试——使用 mock Channel 实现验证 I/O 循环引擎
- [ ] 10.2 为 `PluginHost` 编写单元测试——验证插件生命周期（注册、初始化、停止）
- [ ] 10.3 为 `TransferManager` 编写单元测试——验证三策略选择逻辑
- [ ] 10.4 为 `CredentialStore` 编写单元测试——验证存储/检索/删除和降级后端
- [ ] 10.5 端到端测试——串口连接、SSH 连接、YModem 传输、SFTP 传输
- [ ] 10.6 跨平台构建验证——Windows、macOS、Linux 三平台 `tauri build` 成功
