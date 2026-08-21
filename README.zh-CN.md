# TauTerm

> **一台终端，服务机房与实验台。**  
> 面向网络工程师与嵌入式开发者的开源 SSH/SFTP、串口与网络调试工作台，基于 Rust + Tauri 构建。

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE)
[![Release](https://img.shields.io/github/v/release/hamburger-os/TauTerm?include_prereleases)](https://github.com/hamburger-os/TauTerm/releases)
[![Windows](https://img.shields.io/badge/Windows-download-0078D4)](https://github.com/hamburger-os/TauTerm/releases)
[![Tauri v2](https://img.shields.io/badge/Tauri-v2-67D6F8.svg)](https://tauri.app)
[![Rust](https://img.shields.io/badge/Rust-powered-000000.svg)](https://www.rust-lang.org/)

**[下载 Windows 版](https://github.com/hamburger-os/TauTerm/releases)** · **[macOS/Linux 从源码构建](docs/BUILDING.md)** · **[English README](README.md)**

TauTerm 面向那些不想再在 SSH 客户端、SFTP、串口助手、TCP/UDP 调试器之间来回切换的工程师。它把这些工作流放进一个轻量桌面应用，并通过微内核插件架构持续扩展。

> **发布状态：** Windows 已提供安装包；macOS/Linux 当前可从源码构建，预编译包正在准备。下方功能描述以当前 `master` 为准，最新打包版本可能会落后于 `master`。

<!-- 下一次公开发布前，请用真实 TauTerm 主界面截图/GIF 替换这条注释。 -->

---

## 为什么是 TauTerm？

| 你的需求 | TauTerm 提供 |
|---|---|
| 管理服务器 | SSH 终端 + 同连接 SFTP |
| 调试嵌入式设备 | 串口、HEX/Text/Dual、X/Y/ZModem |
| 做网络联调 | TCP/UDP 客户端与服务端、TFTP、Telnet、iPerf |
| 自动化重复操作 | 每会话 Lua 5.4 脚本、自动应答规则 |
| 少开几个工具 | 统一会话、日志、命令面板与快捷键 |
| 后续扩展协议 | 微内核 + 独立协议插件 |

**体积小。** v0.4.0 Windows 安装包约 **8 MB**，且已包含 com0com 虚拟串口驱动。

**同时服务两个场景。** TauTerm 不把串口当成附属功能，而是从一开始就同时面向网络工程与嵌入式开发。

**真正开源。** TauTerm 采用 MIT / Apache-2.0 双许可证，并围绕可独立扩展的协议插件设计。

---

## 亮点功能

### 🖥️ 网络工程

- **SSH + SFTP 单会话** —— 终端与文件传输复用同一条已认证连接。
- **SFTP 文件管理器** —— 浏览、上传/下载、重命名、批量删除、查看远端文件信息。
- **网络调试（TCP/UDP）** —— TCP 客户端/服务端、UDP 客户端/服务端，多对端处理、TEXT/HEX 视图、目标选择与统计。
- **TFTP** —— 客户端/服务端、RFC 7440 窗口、CRC32 校验和重试控制。
- **Telnet** —— RFC 854 协商、窗口大小同步和 keepalive。
- **iPerf2 / iPerf3** —— TCP/UDP 测试、双向模式、实时速率曲线和历史。
- **远程 journald 查看器** —— 通过 SSH 流式追踪或查询日志，支持过滤和 JSON 导出。

### 🔌 嵌入式开发

- **RS-232/485 串口**，Windows 下可选 **虚拟 COM 桥接**（com0com）。
- **XModem / YModem / ZModem**，可直接从活动串口会话执行传输。
- **Text / HEX / Dual 显示**，支持时间戳、分帧和 TX/RX 区分。
- **字符集转码** —— UTF-8、GBK、GB18030、Big5、Shift-JIS、EUC-JP、EUC-KR、ISO-8859-1。
- **四模式发送栏** —— 手动发送、指令面板、自动应答、脚本。
- **嵌入式 Lua 5.4** —— 每会话独立 VM，支持原始字节与按会话编码发送。
- **开发工具箱** —— CRC、Base64/HEX、浮点/大小端、位操作、C `sizeof`、Modbus、AT 指令解析。

### ⚡ 日常工作流

- 统一标签页会话与离线连接配置。
- 系统原生凭据存储与加密文件降级方案。
- SSH 主机指纹确认与日志脱敏。
- 终端搜索、命令面板、全快捷键重绑定。
- 会话日志分卷与过期清理。
- 中文 / 英文运行时切换。
- 三套 Liquid Glass 主题与 Tauri v2 原生级外壳。

---

## 安装

### Windows

从 **[GitHub Releases](https://github.com/hamburger-os/TauTerm/releases)** 下载最新安装包。

Windows 安装包会附带并自动安装开源 [com0com](https://com0com.sourceforge.net/) 虚拟串口驱动，因此虚拟 COM 桥可以开箱即用。com0com 是独立的第三方 GPL 组件。

### macOS / Linux

预编译包正在准备中。当前请按 **[docs/BUILDING.md](docs/BUILDING.md)** 从源码构建。

---

## 已发布功能与 `master` 有什么区别？

TauTerm 迭代较快。想稳定体验请优先使用打包版本；`master` 会包含尚未正式发布的新功能。

- **v0.4.0 — First Public Tech Preview：** 串口、SSH/SFTP、传输子系统、Lua/自动应答工作流、终端搜索、日志、设置、国际化，以及核心微内核/插件架构。
- **当前 `master`：** 持续增加网络、安全与工作流能力，包括 TCP/UDP 网络调试会话。
- **路线图：** 本地 Shell、SSH 隧道/跳板机、会话分组、FTP、录制/分屏、插件 SDK 等。

每个版本的准确变化请查看 **[CHANGELOG.md](CHANGELOG.md)**。

---

## 协议支持矩阵

| 协议 | 当前 `master` | 内容 | 传输 / 角色 |
|---|---:|---|---|
| **Serial**（RS-232/485） | ✅ | terminal | X/Y/ZModem inline |
| **SSH** | ✅ | terminal | SFTP side-channel |
| **TFTP** | ✅ | custom | client + server |
| **Telnet** | ✅ | terminal | RFC 854 |
| **iPerf2 / iPerf3** | ✅ | custom | 网络测速 |
| **网络调试**（TCP/UDP） | ✅ | custom | client + server |
| **本地 Shell**（PTY） | 📋 规划中 | terminal | v0.6 目标 |
| **FTP** | 📋 规划中 | file browser | v0.7 目标 |
| **TRDP** | 📋 规划中 | terminal | v1.0 目标 |

---

## 路线图

```text
v0.5  凭据与安全强化
v0.6  本地 Shell、会话分组、SSH 隧道 + 跳板机
v0.7  网络调试完善、FTP、录制、分屏
v1.0  GA：Windows 完整验证、macOS/Linux 核心可用、
      性能预算、插件 SDK 文档、TRDP
v1.1  「终端 + 示波器」：WebGL 绘图、FFT、
      FireWater/JustFloat 兼容
```

路线图版本是目标而不是承诺；会根据真实用户反馈调整优先级。

---

## 贡献与反馈

TauTerm 仍处在早期阶段，因此现在的用户反馈尤其有价值。

- 发现可复现问题？**[提交 Bug](https://github.com/hamburger-os/TauTerm/issues/new/choose)**。
- 缺少某个让你无法从现有工具迁移的功能？**[提交功能建议](https://github.com/hamburger-os/TauTerm/issues/new/choose)**。
- 想贡献代码？阅读 **[CONTRIBUTING.md](CONTRIBUTING.md)** 与 **[docs/BUILDING.md](docs/BUILDING.md)**。
- 想了解内部设计？查看 **[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)**。

如果 TauTerm 对你有用，给仓库点一个 Star 能帮助更多网络与嵌入式工程师发现它。

---

## 维护者：如何发布与宣传 TauTerm

可复用的版本发布/宣传清单见 **[docs/LAUNCH.md](docs/LAUNCH.md)**，包括截图、Release Notes、GitHub 元数据、Show HN、Reddit、V2EX 与发布后反馈闭环。

---

## 许可证

TauTerm 可按 **MIT License** 或 **Apache License 2.0** 任一许可证使用。

Windows 安装包附带 [com0com](https://com0com.sourceforge.net/) 作为独立第三方 GPL 组件，不影响 TauTerm 自身双许可证。

---

**TauTerm —— 一台终端，服务机房与实验台。**
