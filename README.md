# TauTerm — 跨平台全功能终端模拟器

> **精致、快速、无限扩展** — 面向未来的下一代跨平台全功能终端模拟器。

基于 **Tauri v2**（Rust + React + TypeScript）构建的微内核架构跨平台终端模拟器。内核不包含任何协议实现——所有会话类型（串口、SSH、Telnet、TCP Raw、TRDP、本地 Shell、FTP、iPerf3 等）均作为**独立插件**注册到内核，实现真正的协议无关架构。

---

## 架构总览

```mermaid
graph TB
    subgraph Microkernel["TauTerm Microkernel"]
        direction LR
        WM[Window Manager] --- TH[Tab Host] --- IPC[IPC Bridge] --- CS[Config Store]
        PH[Plugin Host] --- TE[Theme Engine] --- SE[Shortcut Engine] --- I18N[i18n Engine]
    end

    Microkernel -->|"Plugin Registry"| Registry["Plugin Registry"]

    Registry --> S1[Serial<br/>Plugin]
    Registry --> S2[SSH<br/>Plugin]
    Registry --> S3[Telnet<br/>Plugin]
    Registry --> S4[TCP Raw<br/>Plugin]
    Registry --> S5[TRDP<br/>Plugin]
    Registry --> S6[Shell Local<br/>Plugin]
    Registry --> S7[FTP<br/>Plugin]
    Registry --> S8[iPerf3<br/>Plugin]

    S1 ~~~ S2 ~~~ S3 ~~~ S4
    S5 ~~~ S6 ~~~ S7 ~~~ S8
```

### 设计原则

| 原则 | 说明 |
|------|------|
| **内核不含协议** | 12 个内核模块提供平台能力（窗口、标签页、IPC、配置、插件、主题、快捷键、国际化、插件适配、会话存储、日志引擎、日志写入），不包含任何会话类型逻辑 |
| **一切皆插件** | 每个协议和功能都是独立插件，通过 `ProtocolAdapter` trait 和 `registerPlugin()` API 注册 |
| **统一标签页** | 所有会话类型共享同一套标签栏，通过 `content_type` 适配器动态切换内容视图 |
| **策略自适应** | 传输、I/O、安全策略根据会话协议自动选择，无需用户干预 |

---

## 插件架构

### 插件清单

每个插件通过 `manifest.json` 声明元数据：

```json
{
  "id": "ssh",
  "name": "SSH",
  "version": "1.0.0",
  "category": "terminal",
  "icon": "ssh",
  "content_type": "terminal",
  "capabilities": ["connection", "transfer", "authentication", "credential_store", "network_outbound"],
  "transfer_protocols": ["sftp"],
  "config_schema": { /* JSON Schema */ }
}
```

### 后端核心 Trait

```rust
/// 任何协议插件必须实现此 trait（async，基于 #[async_trait]）
#[async_trait]
pub trait ProtocolAdapter: Send + Sync {
    async fn connect(&self, endpoint: &str, params: &Value) -> Result<ProtocolConnection, SessionError>;
    fn content_type(&self) -> ContentType;
    fn io_strategy(&self) -> IoStrategy;
    fn transfer_protocols(&self) -> Vec<TransferProtocolType>;
    fn discover_endpoints(&self) -> Result<Vec<EndpointInfo>, SessionError>;
    // teardown_delay() 等其他方法
}

/// 同步 I/O 通道 —— 串口等阻塞式协议实现此 trait（由 spawn_sync_io_loop 驱动）
pub trait Channel: Read + Write + Send {
    fn is_connected(&self) -> bool;
    fn set_timeout(&mut self, dur: Duration) -> Result<(), ChannelError>;
    fn try_handoff(&mut self) -> Option<Box<dyn Any>>;  // Inline 传输所有权交出
}

/// 异步 I/O 通道 —— SSH 等 tokio 协议实现此 trait（由 spawn_async_io_loop 驱动）
#[async_trait]
pub trait AsyncChannel: Send {
    async fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize>;
    async fn write(&mut self, buf: &[u8]) -> std::io::Result<usize>;
    async fn flush(&mut self) -> std::io::Result<()>;
    fn is_connected(&self) -> bool;
    fn set_timeout(&mut self, dur: Duration) -> Result<(), ChannelError>;
    async fn resize_pty(&mut self, cols: u32, rows: u32) -> Result<(), ChannelError>;
    fn try_handoff(&mut self) -> Option<Box<dyn Any>>;  // 默认 None（SSH 用 SideChannel 策略）
}
```

`ProtocolConnection` 返回 `ChannelKind::Sync(Box<dyn Channel>)` 或 `ChannelKind::Async(Box<dyn AsyncChannel>)`，
并可携带 `side_channel: Option<Arc<dyn SideChannel>>`（如 SSH 的 `SshSideChannel` 持有 russh Handle + SFTP 缓存）。

### 前端注册 API

```typescript
registerPlugin({
  id: 'ssh',
  manifest: { /* manifest.json */ },
  connectForm: SshConnectForm,         // 连接配置组件
  toolbarItems: [...],                 // 活跃时工具栏注入
  contextMenuItems: [...],             // 右键菜单扩展
  bottomPanels: [...],                 // 底部面板标签页
  statusBarItems: [...],               // 状态栏注入
  locales: { 'zh-CN': {...}, 'en-US': {...} },
});
```

### 能力声明

| 能力 | 描述 |
|------|------|
| `connection` | 可建立/断开连接 |
| `transfer` | 支持文件传输 |
| `endpoint_discovery` | 可枚举可用端点 |
| `stream` | 提供二进制数据流 |
| `authentication` | 需要认证（密码/密钥/证书） |
| `credential_store` | 需要访问凭据存储 |
| `filesystem_access` | 需要访问本地文件系统 |
| `network_outbound` | 需要出站网络连接 |
| `network_listen` | 需要监听端口（如 FTP active mode / iPerf3 server） |

### 生命周期

```mermaid
stateDiagram-v2
    direction LR
    [*] --> Discover
    Discover --> Load
    Load --> Initialize
    Initialize --> Ready
    Ready --> Stop
    Stop --> Unload
    Unload --> [*]
```

---

## I/O 架构

### 双模 I/O 策略

不是所有协议都需要 async runtime。内核提供两种 I/O 执行器，插件声明自己需要的模式：

| 模式 | 运行时 | 适用协议 | 特点 |
|------|--------|---------|------|
| **Sync** | `std::thread` | Serial | 低延迟，无 runtime 开销（`serialport` crate 阻塞式 API + Inline 传输 `try_handoff` 模式） |
| **Async** | `tokio` | SSH | 高并发，线程安全（russh 纯 Rust async SSH 库，SFTP 与终端 I/O 并发复用同一会话） |

### 传输子系统

根据会话协议自动选择传输策略：

```mermaid
graph TD
    TM[TransferManager<br/>策略自动选择]
    TM --> Inline[Inline 策略<br/>串口移交<br/>YModem / XModem / ZModem]
    TM --> SideChannel[SideChannel 策略<br/>SSH 子通道<br/>SFTP]
    TM --> Separate[SeparateConnection 策略<br/>独立连接<br/>FTP]
```

---

## 内容适配器

统一标签栏根据 `content_type` 动态渲染内容区域：

| content_type | 渲染器 | 典型插件 |
|-------------|--------|---------|
| `terminal` | xterm.js 实例池（CSS opacity 切换） | Serial, SSH, Telnet, TCP Raw, TRDP, Shell Local |
| `file_browser` | 双栏文件树 + 传输进度 | FTP, NFS |
| `stats_dashboard` | 实时图表/仪表盘 | iPerf3, UDP Monitor |
| `custom` | 插件自定义组件 | 任意 |

---

## 安全模型

```mermaid
graph LR
    subgraph CS[凭据存储 Credential Store]
        PW[密码<br/>加密]
        KEY[SSH 密钥<br/>加密]
        CERT[证书/Token<br/>加密]
    end

    CS -->|"主后端"| Keyring[keyring-rs<br/>macOS Keychain<br/>Windows Credential Manager<br/>Linux Secret Service]
    CS -.->|"降级"| AES[AES-256-GCM<br/>加密文件]
```

- **主机密钥验证**: SSH `known_hosts` 管理，首次连接指纹确认，密钥变更安全警告
- **TLS 证书固定**: TRDP / Telnet TLS 连接证书校验
- **日志脱敏**: 自动过滤密码、私钥、Token，输出 `[REDACTED]`
- **代理转发控制**: SSH Agent Forwarding 默认禁用，需要显式确认

---

## 协议支持矩阵

| 协议 | 状态 | 内容类型 | 传输支持 | I/O 模式 |
|------|------|---------|---------|---------|
| **Serial** (RS-232/485) | ✅ 已实现 | terminal | YModem / XModem / ZModem (Inline) | Sync |
| **SSH** | ✅ 已实现 | terminal | SFTP (SideChannel) | Async |
| **Telnet** | 📋 计划中 | terminal | — | Async |
| **TCP Raw** | 📋 计划中 | terminal | — | Async |
| **TRDP** | 📋 计划中 | terminal | — | Async |
| **Shell Local** (PTY) | 📋 计划中 | terminal | — | Sync |
| **FTP** | 📋 计划中 | file_browser | FTP (SeparateConnection) | Async |
| **iPerf3** | 📋 计划中 | stats_dashboard | — | Async |
| **NFS** | 🔮 远期 | file_browser | NFS (SeparateConnection) | Async |
| **UDP Monitor** | 🔮 远期 | stats_dashboard | — | Async |

---

## 功能特性

- 🔌 **微内核插件架构** — 所有协议作为独立插件，新协议无需修改内核代码
- 🗂️ **统一标签页管理** — 串口、SSH、FTP、iPerf 共享同一标签栏，拖拽排序，右键菜单
- 🗂️ **离线会话配置** — 不连接即可创建/编辑会话参数，持久化保存，右键菜单一键连接
- 🖥️ **终端仿真** — 基于 xterm.js，多实例池管理，CSS opacity 无重建切换；支持 Text / HEX / Dual 三种数据模式
- 📁 **文件传输** — 右侧竖向面板，按会话独立配置传输协议（YModem/XModem/ZModem），Inline / SideChannel / SeparateConnection 多策略自适应
- 📊 **Dual 双模显示** — 可拖拽分栏同时展示 ASCII 文本与 HEX 十六进制，毫秒级时间戳、按 `\r\n`/`\n`/`\r` 自动分帧、TX/RX 颜色区分
- 📤 **发送栏** — 四模式发送：基础发送 (Text/HEX, 换行符, 循环发送, 历史记录)、指令面板 (预定义命令序列, 拖拽排序, 循环执行)、自动应答 (可视化规则配置, 5 种匹配模式, 10 种动态宏, 定时触发)、脚本编辑器 (嵌入式 Lua 5.4 运行时, 代码生成与手写双路径)；支持后台持续执行，切换会话不中断
- 🤖 **自动应答/脚本引擎** — 嵌入式 Lua 5.4 运行时，每会话独立 VM 沙箱隔离；可视化规则配置编译为 Lua 脚本；支持 5 种匹配模式 (contains/equals/starts_with/regex/lua_pattern)、10 种动态宏、匹配/定时触发、冷却控制、HEX 二进制匹配；"转换为脚本"一键从规则升级为脚本编辑
- ⚙️ **设置页面** — 7 面板全屏覆盖层：通用数据模式、外观（主题 / 字体 / 行缓冲）、语言、编码、日志、快捷键、关于；字体大小和行缓冲滑块拖动即实时生效
- 🔐 **凭据存储** — OS 原生 keyring + AES-256-GCM 降级，密码/密钥/证书/Token 类型安全
- 🎨 **Liquid Glass v3 设计系统** — 动态炫彩光球背景、SVG 噪点磨砂纹理、不对称高光边框、Framer Motion 动画、Google Glow / Obsidian / Frosted 三主题
- 🌐 **多语言** — i18next 命名空间隔离，插件自带翻译，运行时切换
- ⚡ **命令面板** — `Ctrl+Shift+P` 模糊搜索所有命令，键盘驱动操作
- 🔍 **终端搜索** — `Ctrl+F` 搜索 buffer，大小写切换，上下导航
- 🧰 **嵌入式开发工具** — 可折叠右侧栏集成校验和计算（CRC8/16/32 多预设）、编码转换（Base64/HEX/浮点/大小端）、位操作与 C sizeof 计算器、Modbus RTU/ASCII 及 AT 指令协议解析器
- 📋 **日志系统** — 系统事件日志（`TauTerm_YYYYMMDD.log`）自动记录启动/错误/警告；会话数据日志支持 text/hex/dual 格式化、自动分卷与过期清理；右键菜单一键启停、状态栏实时指示
- 🎹 **快捷键系统** — 全局快捷键注册与匹配，localStorage 持久化自定义绑定，点击录制模式实时改键，冲突检测与动画反馈，一键重置默认值
- 💾 **会话持久化** — 离线创建/编辑会话配置，断开会话保留重连，按会话独立传输开关
- 🔄 **虚拟串口桥接** — 连接时自动创建 COM 端口对（com0com 内核驱动），物理串口数据双向桥接到外部工具（如串口调试助手、PLC 上位机），支持 1-4 对虚拟端口并发，PlugInMode 自动隐藏、三层提权策略、孤儿端口启动时自动清理

---

## 技术栈

| 层级 | 技术 |
|------|------|
| 应用框架 | Tauri v2 (Rust) |
| 前端框架 | React 18 + TypeScript |
| 构建工具 | Vite |
| 终端引擎 | xterm.js |
| 动画引擎 | Framer Motion |
| 异步运行时 | tokio |
| 国际化 | i18next + react-i18next |
| 样式方案 | CSS Modules + CSS 自定义属性 |
| 安全存储 | keyring-rs + AES-256-GCM |
| 网络协议 | russh (纯 Rust async SSH) + russh-sftp |
| 脚本引擎 | mlua 0.10 (Lua 5.4, vendored) |
| 正则引擎 | regex 1 |

---

## 项目结构

```
TauTerm/
├── src-tauri/src/
│   ├── kernel/                 # 微内核模块
│   │   ├── mod.rs
│   │   ├── window_manager.rs   # 窗口生命周期、分屏、布局持久化
│   │   ├── tab_host.rs         # 标签页 CRUD、会话关联
│   │   ├── ipc_bridge.rs       # Tauri 命令路由、事件总线、Stream 通道
│   │   ├── config_store.rs     # 类型安全 KV 存储、Schema 校验
│   │   ├── plugin_host.rs      # 插件发现、加载、生命周期
│   │   ├── theme_engine.rs     # CSS 令牌生成、三主题切换（Google Glow / Obsidian / Frosted）
│   │   ├── shortcut_engine.rs  # 快捷键注册、冲突检测、作用域分发
│   │   ├── plugin_adapter.rs   # ProtocolAdapter trait + ContentType/IoStrategy 定义
│   │   ├── session_store.rs    # 会话存储、I/O 生命周期、统计采集
│   │   ├── file_transfer.rs     # 统一文件传输 trait（FileTransfer）+ UnifiedProgress 进度事件 + 取消信号
│   │   ├── i18n_engine.rs      # 命名空间翻译、动态语言切换
│   │   ├── log_engine.rs       # 生产者-消费者异步日志引擎、LogBridge 桥接器
│   │   ├── log_writer.rs       # 日志文件写入器、text/hex/dual 格式化、自动分卷
│   │   ├── comm_handle.rs      # 通信抽象 trait（CommHandle），使脚本引擎协议无关
│   │   ├── data_batcher.rs      # 数据批处理器（16ms 窗口合并高频小包 + Base64 编码优化 IPC）
│   │   └── script_engine/      # Lua 5.4 脚本运行时（VM + 代码生成 + API 注入 + 沙箱）
│   │
│   ├── channel/                # I/O 通道抽象层
│   │   ├── mod.rs              # Channel / AsyncChannel trait + IoStrategy 枚举
│   │   ├── serial_channel.rs   # 串口 Channel 实现（Sync 路径，serialport 阻塞 API）
│   │   ├── ssh_channel.rs      # SSH AsyncChannel 实现（russh::Channel<client::Msg>，PTY 窗口调整）
│   │   ├── tcp_channel.rs      # TCP Channel 实现
│   │   ├── io_loop.rs          # 同步 I/O 循环引擎（spawn_sync_io_loop）
│   │   ├── async_io_loop.rs    # 异步 I/O 循环引擎（spawn_async_io_loop，tokio task）
│   │   ├── serial_comm.rs      # CommHandle 串口适配实现
│   │   └── error.rs            # SessionError 结构化错误
│   │
│   ├── transfer/               # 传输子系统
│   │   ├── mod.rs              # TransferManager + 策略选择
│   │   ├── manager.rs          # 传输策略调度（Inline / SideChannel / SeparateConnection）
│   │   ├── orchestrator.rs     # TransferOrchestrator trait + 策略处理器（Inline / SideChannel）
│   │   ├── panic_guard.rs      # RAII PanicGuard（传输任务 panic 时自动清理会话状态）
│   │   ├── ssh_file_service.rs # SFTP 文件服务（SideChannel 策略，async russh-sftp，复用 SSH Session）
│   │   ├── serial_transfer.rs  # SerialFileTransfer 适配器（spawn_blocking 桥接同步协议到 async FileTransfer trait）
│   │   ├── sftp_transfer.rs    # SftpFileTransfer 适配器（统一 FileTransfer trait 的 async SFTP 实现）
│   │   ├── ymodem.rs           # YModem 协议实现（发送/接收引擎）
│   │   ├── protocol.rs         # 传输协议创建工厂
│   │   └── types.rs            # 传输共享类型
│   │
│   ├── security/               # 安全模块
│   │   └── credential_store.rs # 凭据存储（keyring + AES 降级）
│   │
│   ├── virtual_port/            # 虚拟串口模块（跨平台抽象）
│   │   ├── mod.rs               # 模块声明与 re-export
│   │   ├── backend.rs           # VirtualPortBackend trait（抽象接口，支持 com0com/socat/tty0tty）
│   │   ├── manager.rs           # VirtualPort Manager（com0com 生命周期管理）
│   │   └── bridge.rs            # VirtualPortBridge（后台线程，物理串口 ↔ 虚拟端口双向 I/O）
│   │
│   └── plugins/                # 内建协议插件
│       ├── serial/             # 串口插件（ProtocolAdapter + Channel）
│       └── ssh/                # SSH 插件（ProtocolAdapter + SshSideChannel，密码/密钥认证，SFTP）
│       # Telnet / TCP Raw / TRDP / Shell / FTP / iPerf3 — 计划中
│
├── src/                        # React 前端
│   ├── core/                   # 内核前端 API
│   │   ├── plugin-registry.ts  # registerPlugin() + PluginRegistry
│   │   ├── tab-host.ts         # useTabHost() hook
│   │   ├── config-store.ts     # useConfigStore() hook
│   │   └── event-bus.ts        # 类型事件订阅
│   │
│   ├── renderers/              # 内容适配器（计划中）
│   │   ├── TerminalRenderer.tsx    # xterm.js 实例池
│   │   ├── FileBrowserRenderer.tsx # 双栏文件树（计划中）
│   │   └── CustomRenderer.tsx      # 插件自定义委托
│   │
│   ├── components/             # UI 组件
│   │   ├── Layout/             # TitleBar, Toolbar, Sidebar, StatusBar, ConnectDialog, ResizeHandle, GoogleGlowBackground
│   │   ├── CommandPalette/     # 命令面板
│   │   ├── SendBar/            # 发送栏（基础发送 + 指令面板 + 自动应答 + 脚本编辑器）
│   │   ├── Transmission/       # 传输侧面板（协议配置 + 发送/接收 + 进度）
│   │   ├── RightSidebar/       # 右侧栏容器（可折叠面板 + ResizeObserver 动画）
│   │   ├── Tools/              # 嵌入式开发工具（校验和/编码/位操作/协议解析）
│   │   ├── Settings/           # 设置页（全屏覆盖层：通用/外观/语言/编码/日志/快捷键/关于）
│   │   ├── FileTransfer/       # 传输子组件（协议选择器、配置表单、进度条，被 Transmission 复用）
│   │   ├── FileManager/        # SFTP 文件管理器（目录浏览、上传/下载、批量、属性、预览、进度）
│   │   └── common/             # Icon（30+ SVG 图标）, GlassPanel, GlassButton, GlassInput, ContextMenu, Toast
│   │
│   ├── profiles/               # 会话 Profile 解析器（按协议提供身份信息与参数展示）
│   │   ├── index.ts            # ProfileResolver 聚合 + dispatch
│   │   ├── types.ts            # SessionProfile 类型定义
│   │   ├── serial.ts          # 串口 Profile
│   │   └── ssh.ts             # SSH Profile
│   │
│   ├── styles/                 # 全局样式
│   │   ├── tokens.css           # CSS 自定义属性（3 套主题令牌）
│   │   └── global.css           # 全局动画（morph/flow）和液态玻璃类
│   │
│   └── plugins/                # 插件前端
│       ├── serial/             # SerialConnectForm, 工具栏, 状态栏
│       └── ssh/                # SSH 插件清单、区域设置
│       # Telnet / FTP / iPerf3 等前端插件 — 计划中
│
└── package.json
```

---

## 快捷键

| 快捷键 | 操作 | 作用域 |
|--------|------|--------|
| `Ctrl+Shift+N` | 新建会话 | 全局 |
| `Ctrl+Shift+W` | 关闭当前标签页 | 全局 |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | 切换标签页 | 全局 |
| `Ctrl+F` | 终端搜索 | Terminal 作用域 |
| `Ctrl+Shift+C` | 复制（终端选中文本） | xterm.js 内置（不可自定义） |
| `Ctrl+Shift+V` | 粘贴（到终端） | xterm.js 内置（不可自定义） |
| `Ctrl+Shift+P` | 命令面板 | 全局 |
| `Ctrl+Shift+B` | 切换左侧栏 | 全局 |
| `Ctrl+Shift+E` | 切换右侧栏（开发工具） | 全局 |
| `Ctrl+Shift+R` | 刷新端口列表 | Application 作用域 |

> 💡 以上可自定义快捷键均可通过 **设置 → 快捷键** 面板进行个性化修改：点击任意行进入录制模式，按下目标组合键即可改键；冲突自动检测并给出动画反馈；支持一键重置为默认值。

---

## 构建与运行

### 环境要求

| 组件 | 版本要求 | 说明 |
|------|---------|------|
| **Node.js** | >= 18 | 前端运行时与包管理器 |
| **Rust** | >= 1.96 (推荐) / >= 1.75 (最低) | 后端编译工具链 |
| **npm** | >= 9 | 随 Node.js 附带 |
| **NSIS** | >= 3.0 | Windows 安装包构建工具（仅 Windows 构建需要） |
| **Windows SDK** | >= 10.0 | 提供 `mt.exe` 用于嵌入 `requireAdministrator` 清单（仅 Windows 构建需要，缺少时仍可构建但需手动以管理员运行） |

> **注意**: Rust 1.96+ 内置 `rust-lld` 链接器，在 Windows 上**无需额外安装 Visual Studio Build Tools**。如果使用较低版本 Rust，则需要安装 VS Build Tools 提供 MSVC 链接器。

---

### Windows 环境安装

#### 1. 安装 Node.js

从 [nodejs.org](https://nodejs.org/) 下载 LTS 版本安装，或使用 winget：

```powershell
winget install --id OpenJS.NodeJS.LTS --source winget
```

验证安装：

```powershell
node --version   # 应输出 >= v18.0.0
npm --version    # 应输出 >= 9.0.0
```

#### 2. 安装 Rust

使用 winget 安装 rustup（Rust 官方工具链管理器）：

```powershell
winget install --id Rustlang.Rustup --source winget
```

安装完成后，**重新打开终端**使环境变量生效，然后设置默认工具链：

```powershell
rustup default stable
```

> **下载慢？** 可设置国内镜像源加速：
> ```powershell
> $env:RUSTUP_DIST_SERVER = "https://mirrors.ustc.edu.cn/rust-static"
> rustup default stable
> ```

验证安装：

```powershell
rustc --version   # 应输出 >= rustc 1.75.0
cargo --version   # 应输出 >= cargo 1.75.0
```

#### 3. 链接器

**使用 Rust 内置的 rust-lld**

Rust 1.96+ 自带 LLVM 链接器 `rust-lld`，无需额外安装，编译器会自动使用。

---

### Linux 环境安装

#### Ubuntu / Debian

```bash
# 安装系统依赖
sudo apt update
sudo apt install -y \
  libwebkit2gtk-4.1-dev \
  libappindicator3-dev \
  librsvg2-dev \
  patchelf \
  libssl-dev \
  libkeyring-dev

# 安装 Node.js（使用 NodeSource）
curl -fsSL https://deb.nodesource.com/setup_22.x | sudo -E bash -
sudo apt install -y nodejs

# 安装 Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
```

#### Fedora / RHEL

```bash
sudo dnf install -y \
  webkit2gtk4.1-devel \
  libappindicator-gtk3-devel \
  librsvg2-devel \
  patchelf \
  openssl-devel

# Node.js 和 Rust 安装同上
```

#### Arch Linux

```bash
sudo pacman -S --needed \
  webkit2gtk-4.1 \
  libappindicator-gtk3 \
  librsvg \
  patchelf \
  openssl
```

---

### macOS 环境安装

```bash
# 安装 Xcode Command Line Tools
xcode-select --install

# 安装 Node.js（使用 Homebrew）
brew install node

# 安装 Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
```

---

### 环境验证

运行以下命令确认所有组件正确安装：

```powershell
# Windows (PowerShell)
node --version   # >= 18
npm --version    # >= 9
rustc --version  # >= 1.75
cargo --version  # >= 1.75

# Linux / macOS
node --version && npm --version && rustc --version && cargo --version
```

---

### 开发模式

```bash
# 克隆仓库
git clone https://github.com/hamburger-os/TauTerm.git
cd TauTerm

# 安装前端依赖
npm install

# 启动开发模式（同时启动 Vite 开发服务器和 Tauri 桌面窗口）
npm run tauri dev
```

- Vite 开发服务器运行在 `http://localhost:1420`
- Tauri 窗口自动打开，支持热更新（前端）和热重载（Rust）

> **首次运行**：`npm run tauri dev` 会编译所有 Rust 依赖（约 200+ crates），需要 **5-15 分钟**（视网络和 CPU 而定）。后续编译将利用缓存，通常只需几秒。

---

### 生产构建

#### Windows（生成 .exe 安装包）

Windows 安装包使用 NSIS 构建，**安装 TauTerm 时会自动安装 com0com 虚拟串口驱动**。

**前置条件：安装 NSIS**

```powershell
# 方式 1: winget 安装
winget install --id NSIS.NSIS --source winget

# 方式 2: 官网下载安装
# https://nsis.sourceforge.io/Download
```

安装后**必须将 NSIS 加入系统 PATH**：

```powershell
# 将 NSIS 目录加入当前终端会话 PATH（每次新开终端需重新执行）
$env:PATH += ";C:\Program Files (x86)\NSIS"

# 永久加入用户 PATH（推荐，新终端自动生效）
[Environment]::SetEnvironmentVariable(
    'PATH',
    [Environment]::GetEnvironmentVariable('PATH', 'User') + ';C:\Program Files (x86)\NSIS',
    'User'
)
```

验证 NSIS 可用：

```powershell
makensis -VERSION   # 应输出版本号，如 v3.10
```

**构建流程**

```bash
npm run tauri:build
```

构建过程自动执行：

1. `check-com0com.js` — 验证 `resources/com0com/x64/` 和 `x86/` 中驱动文件齐全（setupc.exe, com0com.sys, com0com.inf, com0com.cat）
2. `tsc && vite build` — 前端 TypeScript 编译 + Vite 打包
3. `build.rs` — 根据目标架构（x86_64 → x64, i686 → x86）将 7 个驱动文件复制到打包根目录
4. `cargo build --release` — Rust 后端编译
5. **NSIS 打包** — 生成安装程序，内含 com0com 驱动文件 + post-install hook（安装时自动执行 `setupc.exe install`）

**构建产物**：

```
src-tauri/target/release/bundle/nsis/
├── TauTerm_<version>_x64-setup.exe    # NSIS 安装程序（推荐分发）
└── TauTerm_<version>_x64_en-US.msi    # WiX MSI 安装包
```

**安装程序行为**：

- **安装时**：NSIS post-install hook 自动执行 `setupc.exe install` 安装 com0com 内核驱动（安装程序天然以管理员身份运行）
- **卸载时**：NSIS pre-uninstall hook 自动执行 `setupc.exe uninstall` 移除驱动
- **运行时回退**：如果因某种原因驱动未在安装时装好，首次连接串口时会尝试运行时安装（需管理员权限）

> **注意**：必须使用 `npm run tauri:build`（等效于 `npx tauri build --bundles nsis`）来生成安装程序。不加 `--bundles nsis` 时 Tauri v2 可能静默跳过 NSIS 打包，只生成 `tauterm.exe`。

#### Linux / macOS

```bash
npm run tauri build
```

构建产物位于 `src-tauri/target/release/bundle/`：`.deb` / `.rpm` / `.AppImage`（Linux），`.dmg` / `.app`（macOS）。

---

### 使用虚拟串口桥接

> **驱动版本**：使用 com0com v3.0.0.0（GPL 开源内核驱动），支持 Windows 10/11 x64/x86。详细的 com0com 使用与故障排查请参考 [tauterm-com0com skill](.claude/skills/tauterm-com0com/SKILL.md)。

虚拟串口功能**默认开启**，连接串口时自动创建 COM 端口对。基本使用流程：

1. **连接串口**：在连接对话框中，串口配置区域的"启用虚拟串口"开关默认开启，"设备数量"（1-4）决定创建多少对端口
2. **查看端口对**：连接成功后，状态栏显示 `VPort: COM22↔COM23, …`，端口 A（COM22）由 TauTerm 占用，端口 B（COM23）供外部工具打开
3. **外部工具读取**：用任意串口工具（如 SSCOM、Putty、Python `pyserial`）打开端口 B（COM23），即可实时接收物理串口的数据
4. **外部工具写入**：外部工具向端口 B 写入的数据会自动转发到物理串口，实现双向桥接
5. **断开自动清理**：断开串口或关闭 TauTerm 时，自动删除所有虚拟端口对
6. **手动清理残留**：状态栏右侧常驻 `[清理残留端口]` 按钮，点击可触发 UAC 提权批量清理所有已知残留端口对

> **注意**：
> - 首次使用需安装 com0com 内核驱动 — 安装 TauTerm 时由 NSIS 安装程序自动完成
> - 如果驱动因故未安装，状态栏会显示 `VPort 未就绪 — 驱动未安装` 并提供 `[修复]` 按钮（点击将触发 UAC 提权安装）
> - **开发模式**：`npm run tauri dev` 启动的应用无管理员权限，清理操作可能需要 UAC 提权。点击状态栏 `[清理残留端口]` 手动触发，或下次连接时自动由 UAC 批量清理

---

## 发展路线图

### v0.3 — 微内核重构 ✅（当前版本）
- [x] 8 模块微内核架构
- [x] `Channel` trait I/O 抽象
- [x] Plugin Host + `ProtocolAdapter` trait
- [x] Serial 插件化重构
- [x] 统一标签页渲染 + 内容适配器
- [x] 三策略传输子系统
- [x] 虚拟串口桥接
- [x] 嵌入式开发工具（校验和/编码/位操作/协议解析）
- [x] 自定义快捷键面板
- [x] 脚本引擎 (Lua 5.4, per-session VM, 自动应答, 代码生成)

### v0.4 — 网络协议
- [x] SSH 插件（密码/密钥认证，SideChannel 架构）
- [x] SFTP 文件传输（SideChannel 策略，async russh-sftp，SFTP 缓存复用）
- [ ] SSH known_hosts 主机密钥验证
- [ ] SSH Agent Forwarding
- [ ] Telnet 插件（RFC 854 选项协商）
- [ ] TCP Raw 插件

### v0.5 — 凭据 & 安全
- [x] 日志基础设施（系统事件日志 + 会话数据日志）
- [ ] Credential Store（keyring + AES 降级）
- [ ] 日志脱敏
- [ ] 权限提升确认

### v0.6 — 本地 Shell & 工具
- [ ] Shell Local 插件（PTY）
- [ ] TRDP 插件
- [ ] 终端分屏

### v0.7 — 文件管理 & 网络诊断
- [ ] FTP 插件（Active/Passive 模式、文件浏览器视图）
- [ ] iPerf3 插件（客户端/服务器、实时统计仪表盘）
- [ ] 终端会话录制

### v1.0 — 正式版
- [ ] 全平台测试通过
- [ ] 性能优化（启动时间、内存占用、I/O 吞吐）
- [ ] 插件 SDK 文档
- [ ] 安装包签名与分发

### v1.x+ — 生态扩展
- [ ] 动态插件加载（无需重新编译）
- [ ] 插件市场
- [ ] NFS 客户端
- [ ] 多窗口支持
- [ ] 云端会话同步

---

## 与竞品对比

| 特性 | TauTerm | WindTerm | MobaXterm |
|------|---------|----------|-----------|
| 架构 | **微内核插件** | 单体 | 单体 |
| 协议扩展 | `ProtocolAdapter` trait（5 个方法） | 修改源码 | 修改源码 |
| 多会话管理 | 统一标签栏 + 内容适配器 | 标签页 | 标签页 + 侧栏 |
| 传输策略 | Inline / SideChannel / SeparateConn | 有限 | FTP/SFTP 内置 |
| 安全存储 | keyring + AES-256-GCM | 内置加密 | 内置加密 |
| 主题系统 | Liquid Glass v3 + 动态光球背景 + 噪点纹理 + 三主题 | 有限主题 | 有限主题 |
| 跨平台 | Windows / Linux / macOS | Windows / Linux / macOS | Windows |
| UI 框架 | React 18 + Framer Motion | Qt | 原生 Win32 |
| 插件生态 | 计划中 | 无 | 有限（插件） |
| 嵌入式工具 | CRC/编码/位操作/协议解析 | 无 | 无 |
| 串口桥接 | com0com 虚拟端口对 | 无 | 无 |
| 开源协议 | MIT | Apache 2.0 | 部分开源 |

---

## 贡献指南

TauTerm 处于活跃开发阶段，欢迎贡献。

### 开发新插件

1. 在 `src-tauri/src/plugins/` 创建插件目录
2. 实现 `ProtocolAdapter` trait（见上文"后端核心 Trait"章节）
3. 编写 `manifest.json` 声明元数据与能力（参考 Serial 插件 `manifest` 定义）
4. 在 `registerPlugin()` 中注册前端组件（`connectForm`、`locales` 等）
5. 在 Plugin Host（`src-tauri/src/lib.rs` 的 `run()` 函数）中注册插件
6. 在 `commands.rs` 的 `connect_session` match 分支中添加协议路由

详细插件开发指南将在 v1.0 发布时随插件 SDK 文档一同推出。

### 主题开发

所有 UI 组件遵循 **Liquid Glass v3** 设计系统。开发新组件或修改样式时，请参考：

- [主题开发指南](docs/theme-guide.md) — CSS 令牌参考、组件开发规范、检查清单
- [tauterm-theme 技能](.claude/skills/tauterm-theme/SKILL.md) — Claude Code 内强制执行的零硬编码颜色规则

---

## 许可证

MIT License

---

**TauTerm** — 微内核架构驱动的下一代跨平台全功能终端模拟器。
