# TauTerm — 跨平台全功能终端模拟器

基于 **Tauri v2**（Rust + React + TypeScript）构建的现代化跨平台终端模拟器，采用 **Liquid Glass** 设计风格和 **Framer Motion** 动画引擎。

## ✨ 功能特性

- 🔌 **多协议架构** — 统一的 SessionManager 抽象层，当前支持串口，预留 SSH/Telnet/TFTP 扩展
- 🗂️ **多会话管理** — 左侧栏管理所有会话，支持无限数量历史会话，右键上下文菜单（连接/重连/配置/删除）
- 🪟 **全功能新建会话** — 模式优先设计（串口/SSH/Telnet/TFTP），串口模式完整可用，其他模式预览占位
- 🖥️ **终端仿真** — 基于 xterm.js，支持 ANSI 转义序列、彩色输出和光标控制
- 🔍 **终端搜索** — `Ctrl+F` 搜索终端 buffer，支持大小写切换和上下导航
- ⚡ **命令面板** — `Ctrl+Shift+P` 模糊搜索所有命令，键盘驱动操作
- 📁 **文件传输** — 多协议传输框架（YModem/XModem/ZModem），支持批量文件收发、逐文件进度、传输历史和 Drag & Drop，底部面板三列布局（配置｜进度｜历史）
- 🎨 **主题** — Liquid Glass v2 磨砂玻璃面板、霓虹发光边框、Framer Motion 交互动画
- 🌐 **多语言** — 默认简体中文，支持即时切换至英文
- 💾 **会话持久化** — 断开会话保留在侧栏，自动保存/恢复会话配置与参数，重连无需重新配置
- 🎹 **快捷键系统** — 统一注册表 + 冲突检测
- 🚀 **跨平台** — Windows、Linux、macOS

## 🛠️ 技术栈

| 层级 | 技术 |
|------|------|
| 后端框架 | Tauri v2 (Rust) |
| 前端框架 | React 18 + TypeScript |
| 构建工具 | Vite |
| 终端引擎 | xterm.js + FitAddon + WebLinksAddon |
| 动画引擎 | Framer Motion |
| 串口库 | serialport (Rust) |
| 国际化 | i18next + react-i18next |
| 样式方案 | CSS Modules + CSS 自定义属性 (Liquid Glass v2) |

## 🏗️ 架构

```
┌──────────────────────────────────────────────────┐
│  React 前端                                       │
│  ├── AppShell (Context Provider 层)               │
│  ├── Toolbar (新建会话 / 侧栏 / 命令面板 / 设置)    │
│  ├── SessionSidebar (会话列表 + 右键上下文菜单)     │
│  ├── TerminalView (xterm.js 多实例)               │
│  ├── SearchBar (终端内容搜索)                      │
│  ├── CommandPalette (命令面板)                     │
│  ├── BottomPanel (信息面板 + 文件传输，可调高度)     │
│  ├── ConnectDialog (模式选择 → 参数配置，两步流程)   │
│  └── StatusBar (状态栏)                           │
├──────────────────────────────────────────────────┤
│  State: SessionContext + ThemeContext + TransferContext │
├──────────────────────────────────────────────────┤
│  Tauri IPC (invoke + events, 每事件携带 session_id) │
├──────────────────────────────────────────────────┤
│  Rust 后端                                        │
│  ├── SessionManager (多会话生命周期)               │
│  │   └── SessionHandle (独立 I/O 线程 + 缓冲通道)  │
│  ├── SerialSession (串口连接实现)                  │
│  └── transfer/ (YModem 传输协议，多协议扩展预留)     │
└──────────────────────────────────────────────────┘
```

## ⌨️ 快捷键

| 快捷键 | 操作 |
|--------|------|
| `Ctrl+N` | 新建会话 |
| `Ctrl+Shift+W` | 关闭当前会话 |
| `Ctrl+F` | 终端搜索 |
| `Ctrl+Shift+P` | 命令面板 |
| `Ctrl+B` | 切换侧边栏 |
| `Ctrl+Shift+R` | 刷新端口列表 |
| `Ctrl+Shift+C/V` | 复制/粘贴 |

## 📦 构建与运行

### 前置要求

- [Node.js](https://nodejs.org/) >= 18
- [Rust](https://www.rust-lang.org/) >= 1.75
- Windows: Visual Studio Build Tools
- Linux: `libwebkit2gtk-4.1-dev`、`libappindicator3-dev`
- macOS: Xcode Command Line Tools

### 安装依赖

```bash
npm install
```

### 开发模式

```bash
npm run tauri dev
```

### 构建生产版本

```bash
npm run tauri build
```

## 📁 项目结构

```
TauTerm/
├── src-tauri/src/             # Rust 后端
│   ├── main.rs                # 入口点
│   ├── lib.rs                 # 应用初始化 + AppState
│   ├── commands.rs            # Tauri 命令（协议无关）
│   ├── session/
│   │   ├── mod.rs             # TermSession trait + ConnectionType
│   │   ├── manager.rs         # SessionManager（多会话管理）
│   │   └── serial.rs          # SerialSession（串口 + I/O 线程）
│   ├── serial/
│   │   ├── mod.rs
│   │   └── config.rs          # 串口配置类型
│   └── transfer/              # 文件传输协议
│       ├── mod.rs
│       └── ymodem.rs           # YModem 协议实现（发送/接收）
├── src/                       # React 前端
│   ├── App.tsx                # AppInner + 布局集成
│   ├── context/
│   │   ├── SessionContext.tsx  # 会话状态管理
│   │   ├── ThemeContext.tsx    # 主题管理
│   │   └── TransferContext.tsx # 文件传输状态
│   ├── components/
│   │   ├── Layout/            # AppShell, Toolbar, SessionSidebar, BottomPanel, ConnectDialog, ResizeHandle, StatusBar
│   │   ├── Terminal/          # Terminal, TerminalView, SearchBar
│   │   ├── CommandPalette/    # 命令面板
│   │   ├── FileTransfer/      # 文件传输面板（三列布局）
│   │   │   ├── columns/        # ConfigColumn, ProgressColumn, HistoryColumn
│   │   │   ├── protocol-config/# 协议选择器 + Y/X/ZModem 配置表单
│   │   │   ├── progress/       # AggregateProgress, PerFileList, ProgressBar
│   │   │   ├── history/        # HistoryFilters, HistoryItem
│   │   │   └── shared/         # ProtocolBadge, ConnectionStatusDot
│   │   └── common/            # GlassPanel, GlassButton, ContextMenu, Toast
│   ├── hooks/                 # useKeyboard, useContextMenu
│   ├── shortcuts/             # 快捷键注册表
│   ├── i18n/                  # zh-CN / en-US
│   └── styles/                # tokens.css, global.css
└── package.json
```

## 🗺️ 路线图

- [x] 串口终端（枚举、连接、收发数据）
- [x] 多会话侧栏架构 + 会话持久化
- [x] 全功能新建会话对话框（模式优先）
- [x] 右键上下文菜单（连接/重连/配置/删除）
- [x] 文件传输框架（已实现 YModem，预留 XModem/ZModem）+ Drag & Drop
- [x] Liquid Glass v2 设计系统 (Neon Dark / Ocean / Sunset)
- [x] 终端搜索 (Ctrl+F)
- [x] 命令面板 (Ctrl+Shift+P)
- [x] 快捷键系统
- [x] 中/英多语言支持
- [ ] SSH 终端连接
- [ ] Telnet 终端连接
- [ ] TFTP 传输
- [ ] SCP/SFTP 文件传输
- [ ] 终端分屏
- [ ] 终端会话录制
- [ ] 插件系统

## 📄 许可证

MIT License

---

**TauTerm** — 精致、快速、跨平台的全功能终端。
