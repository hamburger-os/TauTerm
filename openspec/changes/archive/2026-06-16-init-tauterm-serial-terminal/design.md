## 背景

TauTerm 是一个基于 Tauri v2 构建的全新跨平台全功能终端模拟器。首发版本聚焦于串口终端功能和 YModem 文件传输，包裹在现代 Liquid Glass（玻璃拟态）UI 之中。

核心架构决策：通过 `TermSession` trait 建立统一的终端会话抽象层，使前端和命令层与具体连接协议解耦。串口只是第一个实现——SSH、Telnet 等连接方式可通过实现相同 trait 无缝接入。

当前状态：首发版本，串口终端 + YModem 文件传输已完成。界面默认语言为中文，支持切换至英文。

## 目标 / 非目标

**目标：**
- 使用 Rust 后端和 React + TypeScript 前端启动 Tauri v2 项目
- 建立 `TermSession` 会话抽象层，为多协议支持奠基
- 实现串口发现、配置和双向数据流（首发连接类型）
- 通过 xterm.js 渲染终端输出，支持 ANSI 转义序列
- 在 Rust 中实现 YModem 文件收发协议
- 应用 Liquid Glass 设计系统：磨砂玻璃面板、模糊、半透明、渐变强调色
- 以 Windows（COM 端口）为主要平台，兼容 Linux/macOS
- 实现中/英多语言支持，界面默认中文

**非目标（首发版本）：**
- SSH/Telnet 终端支持（架构已预留，未来迭代实现）
- ZModem、Kermit 协议支持（trait 可扩展设计已就位）
- 终端会话录制或宏脚本
- 插件系统或主题引擎
- 移动端或 Web 平台支持

## 设计决策

### 1. Tauri v2 + React + TypeScript

**选择：** Tauri v2 配合 React 18 + TypeScript 前端。

**理由：** Tauri v2 提供最小的二进制体积，通过 `serialport` crate 实现 Rust 原生串口访问，以及支持 CSS backdrop-filter 的 webview 实现玻璃拟态效果。

### 2. TermSession 会话抽象层（核心架构决策）

**选择：** 定义 `TermSession` trait，抽象统一的终端连接接口。

```rust
pub trait TermSession: Send {
    fn enumerate_endpoints(&self) -> Result<Vec<EndpointInfo>, String>;
    fn connect(&mut self, endpoint: &str, params: Value, on_data: ..., on_disconnect: ...) -> Result<(), String>;
    fn disconnect(&mut self) -> Result<(), String>;
    fn write(&mut self, data: &[u8]) -> Result<(), String>;
    fn state(&self) -> SessionState;
    fn connection_type(&self) -> ConnectionType;
}
```

**实现：**
- `SerialSession`：串口连接（首发 ✅）
- `SshSession`：SSH 连接（规划中 🚧）
- `TelnetSession`：Telnet 连接（规划中 🚧）

**前端命令层：** 命令通过会话抽象层操作，不感知具体连接类型。前端通过 `ConnectionType` 枚举动态显示对应配置 UI。

**理由：** 这种设计意味着添加新协议只需实现 trait，无需修改命令层、事件系统或前端终端组件。YModem 等文件传输协议也通过此架构与具体连接解耦。

### 3. Rust 原生串口 + 事件驱动 IPC

**选择：** 所有串口操作在 Rust 后端通过 `serialport` crate 进行。数据通过 Tauri 事件（`session-data`、`session-connected`、`session-disconnected`）流式传输到前端。

**理由：** Rust 的 `serialport` crate 成熟、跨平台且支持异步。Tauri 的事件系统提供适合终端数据（高波特率下可能高吞吐量）的低延迟流式传输。

### 3b. 专用 I/O 线程（串口数据完整性）

**问题：** 初始 `Arc<Mutex<>>` 共享端口设计导致读写竞争——高波特率下（如 Tab 自动补全产生大量输出），写操作阻塞读循环，造成数据丢失/乱序。

**修复：** 重构为专用 I/O 线程架构：
- 串口端口由单个后台线程独占
- 读取：非阻塞轮询 → `on_data` 回调
- 写入：通过 `mpsc::channel` 发送到 I/O 线程串行执行
- 取消：`AtomicBool` + `oneshot` 双重信号机制
- 结果：完全消除竞态，Tab 自动补全输出稳定正确

### 4. xterm.js 作为终端渲染引擎

**选择：** xterm.js 配合 `@xterm/addon-fit` 和 `@xterm/addon-web-links` 插件。

**理由：** xterm.js 是 Web 终端的行业标准（用于 VS Code、Hyper、Tabby）。它处理 ANSI 转义序列、光标定位、颜色和剪贴板。

### 5. Rust 实现 YModem

**选择：** 在 Rust 中自定义实现 YModem 协议，基于 YModem 规范（1024 字节块大小、CRC-16，块 0 包含文件元数据）。

**理由：** Rust 生态中没有维护良好的 YModem crate。自定义实现可与串口生命周期紧密集成。设计使用 trait 方案，后续可添加 ZModem/Kermit。

### 6. Liquid Glass 设计系统

**选择：** 纯 CSS 玻璃拟态，使用 `backdrop-filter: blur()`、半透明背景、微细边框和 CSS 过渡。

**设计令牌：**
- 基础背景：深蓝紫渐变（`#0a0a1a` → `#0d1117`）
- 玻璃面板：`rgba(255,255,255,0.03)` 配合 `backdrop-filter: blur(16px)`
- 强调色渐变：青绿到天蓝（`#00d4aa` → `#00a3ff`）
- 文字：主色 `#e0e0e0`，辅色 `#888`
- 字体：终端 JetBrains Mono，UI 使用 Inter（支持中文字体回退）

### 7. 项目结构

```
TauTerm/
├── src-tauri/                # Rust 后端
│   └── src/
│       ├── main.rs           # 入口点
│       ├── lib.rs            # 应用初始化 + 会话管理
│       ├── commands.rs       # Tauri 命令（协议无关）
│       ├── session/          # 终端会话抽象层 ★核心架构★
│       │   ├── mod.rs        # TermSession trait + ConnectionType
│       │   └── serial.rs     # SerialSession 实现
│       ├── serial/           # 串口低级操作（向后兼容）
│       └── transfer/         # 文件传输协议
│           ├── protocol.rs   # FileTransferProtocol trait
│           └── ymodem.rs     # YModem 发送/接收/CRC-16
├── src/                      # React 前端
│   ├── App.tsx               # 应用根组件
│   ├── components/
│   │   ├── Terminal/         # xterm.js 终端
│   │   ├── Sidebar/          # 连接配置（支持类型切换）
│   │   ├── FileTransfer/     # 文件传输面板
│   │   └── common/           # GlassPanel, Toast 等
│   ├── hooks/                # useSerialPort, useFileTransfer
│   ├── i18n/                 # 多语言国际化
│   └── styles/               # 设计令牌 + 全局样式
├── README.md
└── package.json
```

### 8. 多语言国际化（i18n）

**选择：** 使用 `react-i18next` + `i18next` 实现前端国际化。

**理由：** `react-i18next` 是 React 生态中最广泛使用的国际化方案。语言切换即时生效，无需重启应用。后端返回语言无关的状态码，前端根据事件码映射到对应语言文本。

## 风险 / 权衡

- **串口库平台差异**：→ 缓解：在平台无关的 Rust API 后抽象端口枚举
- **高波特率性能**：→ 缓解：批量读取 4KB 块，共享 Arc<Mutex<>> 端口访问
- **YModem 高延迟链路**：→ 缓解：10 次重试机制，未来优先开发 ZModem（流式传输）
- **backdrop-filter 浏览器支持**：→ 缓解：优雅降级——不支持的浏览器回退到不透明深色面板
- **SSH/Telnet 依赖**：→ 缓解：TermSession trait 已设计，实现新协议不影响现有代码
