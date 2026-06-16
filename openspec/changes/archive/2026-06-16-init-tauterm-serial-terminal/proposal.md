## 为什么

类似 WindTerm 和 MobaXterm 的终端模拟器功能强大，但大多是闭源、仅限 Windows 或缺乏现代化的 UI 设计。TauTerm 旨在成为一个基于 Tauri（Rust + Web 前端）构建的跨平台、开源的全功能终端模拟器。

首发版本从稳健的串口终端支持和 YModem 文件传输起步——这是嵌入式开发者、网络工程师和系统管理员的核心工具。架构设计通过统一的 `TermSession` 会话抽象层，支持未来无缝扩展 SSH、Telnet 等连接方式。

## 变更内容

- 初始化一个名为"TauTerm"的 Tauri v2 项目，包含 Rust 后端和 Web 前端
- 建立项目配置：`.gitignore`、`README.md`、`LICENSE` 以及会话抽象架构
- 在 Rust 后端实现终端会话抽象层（`TermSession` trait），统一串口/SSH/Telnet 接口
- 实现串口连接管理（打开、配置波特率/校验位/停止位/数据位、关闭）
- 使用 xterm.js 构建终端仿真前端，实时渲染串口数据
- 在 Rust 后端实现 YModem 文件收发协议（CRC-16 校验、重试机制）
- 设计并实现 Liquid Glass（玻璃拟态）UI 主题：磨砂玻璃面板、模糊效果、半透明叠加、渐变强调色、平滑动画
- 构建清晰现代的布局：连接配置侧边栏（支持连接类型选择）、终端视口、文件传输面板
- 实现多语言（i18n）支持：默认中文界面，可切换为英文

## 功能模块

### 新增功能模块
- `project-scaffold`：初始化 Tauri v2 项目结构、Rust 工作空间、前端框架（React + TypeScript）、会话抽象架构
- `session-architecture`：终端会话抽象层（`TermSession` trait），统一串口/SSH/Telnet 连接接口
- `serial-terminal`：串口连接实现——端口枚举、连接配置、异步数据流传输到 xterm.js 终端 UI
- `file-transfer`：基于活跃串口连接的 YModem 批量文件收发，带进度指示和 CRC-16 错误校验
- `liquid-glass-ui`：玻璃拟态设计系统——磨砂玻璃面板、暗色主题、渐变强调色、流畅动画
- `i18n`：多语言国际化支持——默认中文，可切换英文

### 未来规划的模块
- `ssh-session`：SSH 终端连接（TermSession trait 实现）
- `telnet-session`：Telnet 终端连接（TermSession trait 实现）
- `zmodem-transfer`：ZModem 文件传输协议
- `session-recording`：终端会话录制与回放

## 影响范围

- **新项目**：全新的 Tauri 应用——无需修改现有代码
- **依赖项**：Tauri v2、Rust serialport crate、xterm.js、React + TypeScript、CSS 玻璃拟态效果、react-i18next
- **目标平台**：Windows（主要，COM 端口）、Linux（ttyUSB/ttyACM）、macOS（cu.usbmodem）
- **代码仓库**：`c:/workspace/Tauri/TauTerm`
- **语言**：界面默认中文，支持切换英文；文档和注释使用中文
