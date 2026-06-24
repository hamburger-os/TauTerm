## Why

TauTerm 当前是串口终端模拟器，架构硬编码了 `SerialPort` 依赖——`spawn_io_thread` 绑死串口类型，`connect_session` 无法创建非串口会话，传输端口移交仅适用于物理串口。要成为与 WindTerm / MobaXterm 正面竞争的跨平台全功能终端，需要从根本上重构为**微内核插件架构**：内核不包含任何协议实现，所有会话类型（Serial、SSH、Telnet、TCP Raw、TRDP、Shell、FTP、NFS、iPerf、UDP 等）均作为插件注册到内核。现在是重构的最佳时机——代码量尚小，串口是唯一实现，重构代价最低，收益最大。

## What Changes

- **BREAKING**: 重构 Rust 后端为 8 模块微内核（Window Manager、Tab Host、IPC Bridge、Config Store、Plugin Host、Theme Engine、Shortcut Engine、i18n Engine），内核不含任何协议实现
- **BREAKING**: `SessionManager` 拆分为 `TabHost`（标签页生命周期）+ `PluginHost`（插件注册/发现），当前 `SessionImpl` 枚举 → `ProtocolAdapter` trait
- **BREAKING**: `spawn_io_thread(Box<dyn SerialPort>)` → `spawn_io_loop(Box<dyn Channel>)`，`Channel` trait 统一抽象所有 I/O 通道（串口、TCP、SSH channel、Pipe、UDP socket）
- **BREAKING**: 传输子系统从串口移交单一策略 → 三策略架构（Inline / SideChannel / SeparateConnection），支持 YModem/XModem/ZModem + SCP/SFTP + FTP
- **新增**: 插件系统——manifest 定义 + `ProtocolAdapter` trait + 能力声明 + 生命周期管理，Serial 作为首个内建插件
- **新增**: 凭据存储（Credential Store）——keyring 后端 + AES-256-GCM 降级，类型安全的密码/密钥/证书管理
- **新增**: 统一标签页渲染——`content_type` 适配器，根据会话类型动态切换 Terminal / FileBrowser / StatsDashboard / Custom 视图
- **新增**: 前端插件注册 API——`registerPlugin()` 统一入口，插件提供连接表单、工具栏按钮、右键菜单、底部面板、状态栏项
- **新增**: 双模 I/O 策略——同步模式（`std::thread`）用于串口/TCP Raw，异步模式（`tokio`）用于 SSH/Telnet/HTTP
- **修改**: `ConnectDialog` 从硬编码模式卡片 → 从 Plugin Registry 动态生成协议选项
- **修改**: 会话持久化从 ad-hoc JSON → Config Store（类型安全 + schema 校验）
- **修改**: 错误处理从 `String` → 结构化 `SessionError` 枚举

## Capabilities

### New Capabilities

- `microkernel-core`: 8 模块微内核——Window Manager、Tab Host、IPC Bridge、Config Store、Plugin Host、Theme Engine、Shortcut Engine、i18n Engine，内核不包含任何协议实现或业务 UI
- `plugin-system`: 插件清单（manifest.json）+ `ProtocolAdapter` trait + 能力声明（capabilities.json）+ 插件生命周期（发现→加载→初始化→就绪→停止→卸载）
- `io-channel-abstraction`: `Channel` trait 统一 I/O 通道抽象，双模执行器（sync/async），`spawn_io_loop` 协议无关的 I/O 循环引擎
- `credential-store`: 加密凭据存储，keyring-rs 主后端 + AES-256-GCM 文件降级，支持密码/SSH 密钥/证书/Token 四种凭据类型
- `transfer-subsystem`: 三策略传输架构——Inline（串口协议移交）、SideChannel（SSH SFTP/SCP 子通道）、SeparateConnection（FTP 独立数据连接）
- `unified-tab-rendering`: 统一标签栏 + `content_type` 适配器（terminal/file_browser/stats_dashboard/custom），插件通过 `registerPlugin()` 注册 UI 组件、工具栏按钮、右键菜单、底部面板、状态栏项
- `security-model`: 主机密钥验证（SSH known_hosts）、TLS 证书固定（TRDP/Telnet TLS）、代理转发控制、日志自动脱敏
- `protocol-plugins`: 内建协议插件集——serial（已有，改造为插件）、ssh、telnet、tcp-raw、trdp、shell-local、ftp、iperf3

### Modified Capabilities

- `session-manager`: **BREAKING** — `SessionManager` 拆分为 `TabHost`（内核服务，管理标签页生命周期）+ 各插件的 `ProtocolAdapter` 实现（创建/管理会话）。`SessionImpl` 枚举移除，改为 trait 多态。`create_session` 不再硬编码 `ConnectionType::Serial`，改为通过 Plugin Host 查找对应的 `ProtocolAdapter` 调用 `connect()`
- `new-session-dialog`: **BREAKING** — `ConnectDialog` 不再硬编码模式卡片列表。`ConnectionType::all()` 移除。协议选项从 Plugin Registry 动态生成，每个插件提供自己的 `ConnectForm` React 组件。端点枚举通过插件的 `discover_endpoints()` 能力实现
- `file-transfer`: **BREAKING** — 传输不再仅支持串口端口移交。新增 SideChannel 策略（SSH SFTP）和 SeparateConnection 策略（FTP）。传输命令路由表扩展以支持网络协议。取消通道和进度事件保持兼容

## Impact

- **Rust 后端**: `lib.rs`、`commands.rs`、`session/`、`transfer/` 全部重构。新增 `kernel/`（内核模块）、`plugins/`（内建插件）、`channel/`（I/O 通道抽象）、`security/`（凭据/加密）
- **React 前端**: `SessionContext.tsx`、`TransferContext.tsx`、`ConnectDialog.tsx`、`AppShell.tsx` 重构。新增 `core/`（内核前端 API）、`renderers/`（内容适配器组件）、插件注册机制
- **依赖新增**: `tokio`（async runtime）、`keyring`（凭据存储）、`aes-gcm`（加密降级）、`ssh2`（SSH 协议）、`json-schema`（配置校验）
- **BREAKING 变更**: 所有现有 Tauri 命令签名和事件载荷需要更新以支持 `plugin_id` 参数和新的会话类型标识
- **README.md**: 从功能列表 → 完整的高层次架构设计文档，作为后续软件发展的指导蓝图
