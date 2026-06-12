# TauTerm - 跨平台全功能终端模拟器

基于 **Tauri v2**（Rust + React + TypeScript）构建的现代化跨平台终端模拟器，采用 Liquid Glass（玻璃拟态）设计风格。

首发版本实现串口终端和 YModem 文件传输，架构设计支持未来扩展 SSH、Telnet 等多种连接方式。

## ✨ 功能特性

- 🔌 **多协议架构** — 统一的终端会话抽象层，支持串口（首发）、SSH（规划中）、Telnet（规划中）
- 🖥️ **终端仿真** — 基于 xterm.js，支持 ANSI 转义序列、彩色输出和光标控制
- 📁 **文件传输** — 支持 YModem 协议的批量文件收发，带进度指示和重试机制
- 🎨 **玻璃拟态 UI** — 现代化磨砂玻璃面板、半透明叠加、渐变强调色
- 🌐 **多语言** — 默认简体中文，支持即时切换至英文
- 🚀 **跨平台** — 支持 Windows（COM 端口）、Linux（ttyUSB/ttyACM）和 macOS（cu.usbmodem）
- 🔄 **可调整布局** — 可拖拽调整的侧边栏和文件传输面板
- ⚡ **键盘快捷键** — Ctrl+Shift+F 切换文件传输面板，Ctrl+Shift+R 刷新端口列表

## 🛠️ 技术栈

| 层级 | 技术 |
|------|------|
| 后端框架 | Tauri v2 (Rust) |
| 前端框架 | React 18 + TypeScript |
| 构建工具 | Vite |
| 终端引擎 | xterm.js + FitAddon + WebLinksAddon |
| 串口库 | serialport (Rust) |
| 国际化 | i18next + react-i18next |
| 样式方案 | CSS Modules + CSS 自定义属性 (玻璃拟态) |

## 🏗️ 架构设计

```
┌─────────────────────────────────────────┐
│  React 前端 (TypeScript + xterm.js)      │
│  ├── Terminal (终端渲染)                 │
│  ├── Sidebar (连接配置)                  │
│  └── FileTransferPanel (文件传输)        │
├─────────────────────────────────────────┤
│  Tauri IPC (invoke + events)             │
├─────────────────────────────────────────┤
│  Rust 后端                               │
│  ├── session/ (会话抽象层)               │
│  │   ├── TermSession trait (统一接口)    │
│  │   ├── SerialSession (串口实现) ✅     │
│  │   ├── SshSession (规划中) 🚧          │
│  │   └── TelnetSession (规划中) 🚧       │
│  └── transfer/ (文件传输协议)            │
│      └── YModem (发送/接收/CRC-16)       │
└─────────────────────────────────────────┘
```

## 📦 构建与运行

### 前置要求

- [Node.js](https://nodejs.org/) >= 18
- [Rust](https://www.rust-lang.org/) >= 1.75
- 平台相关依赖：
  - **Windows**：Visual Studio Build Tools（或 VS 2022）
  - **Linux**：`libwebkit2gtk-4.1-dev`、`libappindicator3-dev`、`librsvg2-dev`
  - **macOS**：Xcode Command Line Tools

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
├── src-tauri/                # Rust 后端
│   └── src/
│       ├── main.rs           # 入口点
│       ├── lib.rs            # 应用初始化
│       ├── commands.rs       # Tauri 命令（协议无关）
│       ├── session/          # 终端会话抽象层
│       │   ├── mod.rs        # TermSession trait
│       │   └── serial.rs     # 串口会话实现
│       ├── serial/           # 串口低级操作
│       └── transfer/         # 文件传输协议
│           ├── protocol.rs   # FileTransferProtocol trait
│           └── ymodem.rs     # YModem 发送/接收
├── src/                      # React 前端
│   ├── App.tsx               # 应用根组件（布局 + 集成）
│   ├── main.tsx              # 入口
│   ├── components/
│   │   ├── Terminal/         # xterm.js 终端组件
│   │   ├── Sidebar/          # 连接配置侧边栏
│   │   ├── FileTransfer/     # 文件传输面板
│   │   └── common/           # GlassPanel, GlassButton, Toast 等
│   ├── hooks/
│   │   ├── useSerialPort.ts  # 会话连接 hook
│   │   └── useFileTransfer.ts# 文件传输 hook
│   ├── i18n/                 # 多语言国际化
│   │   ├── index.ts
│   │   ├── types.ts
│   │   └── locales/          # zh-CN.json, en-US.json
│   └── styles/               # tokens.css, global.css
├── README.md
└── package.json
```

## 🗺️ 路线图

- [x] 串口终端（枚举、连接、收发数据）
- [x] YModem 文件传输
- [x] Liquid Glass 设计系统
- [x] 中/英多语言支持
- [ ] SSH 终端连接
- [ ] Telnet 终端连接
- [ ] ZModem / Kermit 协议支持
- [ ] 终端会话录制
- [ ] 主题引擎

## 🖼️ 截图

<!-- TODO: 添加应用截图 -->

## 📄 许可证

MIT License

---

**TauTerm** — 精致、快速、跨平台的全功能终端。
