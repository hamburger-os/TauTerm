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
| **内核不含协议** | 8 个内核模块提供平台能力（窗口、标签页、IPC、配置、插件、主题、快捷键、国际化），不包含任何会话类型逻辑 |
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
  "transfer_protocols": ["sftp", "scp"],
  "config_schema": { /* JSON Schema */ }
}
```

### 后端核心 Trait

```rust
/// 任何协议插件必须实现此 trait
pub trait ProtocolAdapter: Send + Sync {
    fn connect(&self, endpoint: &str, params: &Value) -> Result<Box<dyn Channel>, SessionError>;
    fn disconnect(&self, channel: &mut Box<dyn Channel>) -> Result<(), SessionError>;
    fn discover_endpoints(&self) -> Result<Vec<EndpointInfo>, SessionError>;
    fn content_type(&self) -> ContentType;
    fn transfer_protocols(&self) -> Vec<TransferProtocol>;
    fn io_strategy(&self) -> IoStrategy;
}

/// 统一 I/O 通道 —— 所有传输类型实现此 trait
pub trait Channel: Read + Write + Send {
    fn is_connected(&self) -> bool;
    fn set_timeout(&mut self, dur: Duration) -> Result<(), SessionError>;
    fn try_handoff(&mut self) -> Option<Box<dyn Any>>;
}
```

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
| **Sync** | `std::thread` | Serial, TCP Raw, Pipe | 低延迟，无 runtime 开销 |
| **Async** | `tokio` | SSH, Telnet, HTTP, TRDP | 高并发，天然适合网络协议 |

### 传输子系统

根据会话协议自动选择传输策略：

```mermaid
graph TD
    TM[TransferManager<br/>策略自动选择]
    TM --> Inline[Inline 策略<br/>串口移交<br/>YModem / XModem / ZModem]
    TM --> SideChannel[SideChannel 策略<br/>SSH 子通道<br/>SFTP / SCP]
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
| **SSH** | 🔨 开发中 | terminal | SFTP / SCP (SideChannel) | Async |
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
- 🖥️ **终端仿真** — 基于 xterm.js，多实例池管理，CSS opacity 无重建切换
- 📁 **文件传输** — 右侧竖向面板，按会话独立配置传输协议（YModem/XModem/ZModem），Inline / SideChannel / SeparateConnection 多策略自适应
- 📤 **数据发送栏** — 终端底部快速发送，支持 Text/HEX 模式切换、换行符追加、重复发送、发送历史
- ⚙️ **设置页面** — 全屏覆盖层，主题切换 / 字体大小 / 语言 / 字符编码集中管理
- 🔐 **凭据存储** — OS 原生 keyring + AES-256-GCM 降级，密码/密钥/证书/Token 类型安全
- 🎨 **Liquid Glass v2 设计系统** — 磨砂玻璃面板、霓虹发光边框、Framer Motion 动画、Neon Dark / Ocean / Sunset 三主题
- 🌐 **多语言** — i18next 命名空间隔离，插件自带翻译，运行时切换
- ⚡ **命令面板** — `Ctrl+Shift+P` 模糊搜索所有命令，键盘驱动操作
- 🔍 **终端搜索** — `Ctrl+F` 搜索 buffer，大小写切换，上下导航
- 🎹 **快捷键系统** — 全局/插件作用域，冲突检测，作用域分发
- 💾 **会话持久化** — 离线创建/编辑会话配置，断开会话保留重连，按会话独立传输开关

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
| 网络协议 | ssh2 (SSH/SFTP) |

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
│   │   ├── theme_engine.rs     # CSS 变量生成、主题切换
│   │   ├── shortcut_engine.rs  # 快捷键注册、冲突检测、作用域分发
│   │   ├── plugin_adapter.rs   # ProtocolAdapter trait + ContentType/IoStrategy 定义
│   │   ├── session_store.rs    # 会话存储、I/O 生命周期、统计采集
│   │   └── i18n_engine.rs      # 命名空间翻译、动态语言切换
│   │
│   ├── channel/                # I/O 通道抽象层
│   │   ├── mod.rs              # Channel trait 定义
│   │   ├── serial_channel.rs   # 串口 Channel 实现
│   │   ├── tcp_channel.rs      # TCP Channel 实现
│   │   ├── io_loop.rs          # 协议无关 I/O 循环引擎（sync + async）
│   │   └── error.rs            # SessionError 结构化错误
│   │
│   ├── transfer/               # 传输子系统
│   │   ├── mod.rs              # TransferManager + 策略选择
│   │   ├── manager.rs          # 传输策略调度
│   │   ├── ymodem.rs           # YModem 协议实现（发送/接收引擎）
│   │   └── types.rs            # TransferProtocolType 枚举等共享类型
│   │
│   ├── security/               # 安全模块
│   │   └── credential_store.rs # 凭据存储（keyring + AES 降级）
│   │
│   └── plugins/                # 内建协议插件
│       └── serial/             # 串口插件（ProtocolAdapter + Channel）
│       # SSH / Telnet / TCP Raw / TRDP / Shell / FTP / iPerf3 — 计划中
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
│   │   ├── Layout/             # Toolbar, Sidebar, TabBar, StatusBar, ConnectDialog, ResizeHandle
│   │   ├── CommandPalette/     # 命令面板
│   │   ├── SendBar/            # 数据发送栏（文本/HEX 输入、重复发送、发送历史）
│   │   ├── Transmission/       # 传输侧面板（协议配置 + 发送/接收 + 进度）
│   │   ├── Settings/           # 设置页（全屏覆盖层：通用/外观/语言/编码/关于）
│   │   ├── FileTransfer/       # 传输子组件（协议选择器、配置表单、进度条，被 Transmission 复用）
│   │   └── common/             # GlassPanel, GlassButton, ContextMenu, Toast
│   │
│   └── plugins/                # 插件前端
│       └── serial/             # SerialConnectForm, 工具栏, 状态栏
│       # SSH / Telnet / FTP / iPerf3 等前端插件 — 计划中
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
| `Alt+1` ~ `Alt+9` | 快速切换到标签页 N | 全局 |
| `Ctrl+F` | 终端搜索 | Terminal 作用域 |
| `Ctrl+Shift+P` | 命令面板 | 全局 |
| `Ctrl+Shift+B` | 切换侧边栏 | 全局 |
| `Ctrl+Shift+R` | 刷新端口列表 | Serial 作用域 |

---

## 构建与运行

### 前置要求

- **Node.js** >= 18
- **Rust** >= 1.75
- **Windows**: Visual Studio Build Tools
- **Linux**: `libwebkit2gtk-4.1-dev`、`libappindicator3-dev`、`libssh2-dev`、`libkeyring-dev`
- **macOS**: Xcode Command Line Tools

### 开发模式

```bash
npm install
npm run tauri dev
```

### 生产构建

```bash
npm run tauri build
```

---

## 发展路线图

### v0.3 — 微内核重构 ✅（当前版本）
- [x] 8 模块微内核架构
- [x] `Channel` trait I/O 抽象
- [x] Plugin Host + `ProtocolAdapter` trait
- [x] Serial 插件化重构
- [x] 统一标签页渲染 + 内容适配器
- [x] 三策略传输子系统

### v0.4 — 网络协议
- [ ] SSH 插件（密码/密钥/agent 认证、known_hosts）
- [ ] Telnet 插件（RFC 854 选项协商）
- [ ] TCP Raw 插件
- [ ] SFTP/SCP 文件传输（SideChannel 策略）

### v0.5 — 凭据 & 安全
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
| 主题系统 | Liquid Glass v2 + 插件 token 注入 | 有限主题 | 有限主题 |
| 跨平台 | Windows / Linux / macOS | Windows / Linux / macOS | Windows |
| UI 框架 | React 18 + Framer Motion | Qt | 原生 Win32 |
| 插件生态 | 计划中 | 无 | 有限（插件） |
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

---

## 许可证

MIT License

---

**TauTerm** — 微内核架构驱动的下一代跨平台全功能终端模拟器。
