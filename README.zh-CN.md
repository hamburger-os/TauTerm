<p align="center">
  <img src="src/assets/icons/logo.png" width="112" alt="TauTerm Logo">
</p>

<h1 align="center">TauTerm</h1>

<p align="center"><strong>一台终端，服务机房与实验台。</strong></p>

<p align="center">
  面向 SSH/SFTP、串口与 TCP/UDP 网络调试的开源桌面工作台，基于 Rust + Tauri 构建。
</p>

<p align="center">
  <a href="https://github.com/hamburger-os/TauTerm/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/hamburger-os/TauTerm/actions/workflows/ci.yml/badge.svg?branch=master"></a>
  <a href="https://github.com/hamburger-os/TauTerm/releases"><img alt="Release" src="https://img.shields.io/github/v/release/hamburger-os/TauTerm?include_prereleases&label=release"></a>
  <a href="LICENSE"><img alt="License: MIT OR Apache-2.0" src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg"></a>
  <img alt="Rust + Tauri" src="https://img.shields.io/badge/Rust%20%2B%20Tauri-v2-24C8DB">
</p>

<p align="center">
  <a href="https://github.com/hamburger-os/TauTerm/releases"><strong>下载</strong></a>
  · <a href="docs/SUPPORTED_PLATFORMS.md">支持平台</a>
  · <a href="docs/BUILDING.md">从源码构建</a>
  · <a href="docs/ARCHITECTURE.md">架构设计</a>
  · <a href="README.md">English</a>
</p>

TauTerm 把**远程服务器访问、嵌入式设备调试和网络联调**放进同一个轻量桌面应用。它面向那些不想再在 SSH 客户端、SFTP、串口助手、TCP/UDP 调试器之间来回切换的工程师。

> **版本说明：** 本 README 描述当前 `master` 分支。GitHub Releases 是面向终端用户的打包快照，功能可能落后于 `master`。某个版本究竟已经发布了什么，请以 [CHANGELOG.md](CHANGELOG.md) 和对应 Release 页面为准。

![TauTerm：同一会话中的 SSH 终端与 SFTP 文件管理器](docs/assets/hero-zh-CN.png)

---

## 为什么是 TauTerm？

| 你的需求 | TauTerm 提供 |
|---|---|
| 管理服务器 | SSH 终端 + 同一认证连接上的 SFTP |
| 调试嵌入式设备 | RS-232/485 串口、Text/HEX/Dual、X/Y/ZModem |
| 做网络联调 | TCP/UDP 客户端与服务端、TFTP、Telnet、iPerf |
| 自动化重复操作 | 每会话 Lua 5.4 脚本与自动应答规则 |
| 少开几个工具 | 统一会话、日志、命令面板与快捷键 |
| 后续扩展协议 | 面向协议插件的微内核架构 |

TauTerm 从一开始就同时服务**网络工程师**与**嵌入式开发者**。串口不是附属功能，而是与 SSH/SFTP、网络调试并列的一等工作流；不同通信方式共享同一套会话化桌面体验。

---

## 核心工作流

[观看 64 秒静音演示视频](docs/assets/tauterm-demo.mp4)。

### SSH + SFTP

一条已认证的 SSH 连接即可同时承载终端、远程文件和 journald 工作流。SFTP 文件管理器支持浏览、上传/下载、重命名、批量删除和远端文件信息查看。

### 串口与嵌入式开发

![包含文件传输和协议工具的 RT-Thread 串口终端](docs/assets/serial-rtthread-dual-en.png)

使用 RS-232/485 串口完成 Text、HEX 或 Dual 显示，支持时间戳、TX/RX 区分、字符集转码、X/Y/ZModem 传输、Lua 自动化、自动应答，以及常用二进制/协议开发工具。

Windows 可通过安装包自带的 com0com 建立虚拟 COM 对；Linux/macOS 使用 TauTerm 进程内 POSIX PTY 桥接，不依赖 `socat` 或其他外部 helper。

### TCP/UDP 与吞吐测试

![TCP 回环收发日志](docs/assets/network-tcp-loopback-en.png)

在同一个应用内运行 TCP 客户端/服务端与 UDP 客户端/服务端工作流，支持对端选择、逐对端统计、Text/HEX 查看和脚本发送。TFTP、Telnet 与 iPerf2/iPerf3 则补齐同一套网络调试环境。

---

## 亮点功能

### 网络工程

- **SSH + SFTP 单会话** —— 终端与文件传输复用同一条已认证连接。
- **网络调试（TCP/UDP）** —— TCP 客户端/服务端、UDP 客户端/服务端，多对端处理、发送目标选择与统计。
- **TFTP** —— 客户端/服务端、RFC 7440 窗口、CRC32 校验和重试控制。
- **Telnet** —— RFC 854 协商、窗口大小同步和 keepalive。
- **iPerf2 / iPerf3** —— TCP/UDP 测试、双向模式、实时速率曲线与历史记录。
- **远程 journald 查看器** —— 通过 SSH 流式追踪或查询日志，支持过滤和 JSON 导出。

### 嵌入式开发

- **RS-232/485 串口**，支持可选虚拟串口桥接。
- **XModem / YModem / ZModem**，可直接从活动串口会话执行传输。
- **Text / HEX / Dual 显示**，支持时间戳、分帧与 TX/RX 区分。
- **字符集转码** —— UTF-8、GBK、GB18030、Big5、Shift-JIS、EUC-JP、EUC-KR、ISO-8859-1。
- **四模式发送栏** —— 手动发送、指令面板、自动应答、脚本。
- **嵌入式 Lua 5.4** —— 每会话独立 VM，支持原始字节与按会话编码发送。
- **开发工具箱** —— CRC、Base64/HEX、浮点/大小端、位操作、C `sizeof`、Modbus、AT 指令解析。

### 日常工作流

- 统一标签页会话与离线连接配置。
- 终端搜索、命令面板、全快捷键重绑定。
- 会话日志分卷与过期清理。
- 中文 / 英文运行时切换。
- Google Glow、Obsidian、Frosted 三套 Liquid Glass 主题。
- 凭据存储优先使用 OS Keyring，不可用时回退到 Argon2id + AES-256-GCM 加密 vault。

当 TFTP 监听范围超出本机、同时允许远程写入和覆盖时，启动服务前必须显式确认该暴露配置。

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

## 安装

### Windows

从 [GitHub Releases](https://github.com/hamburger-os/TauTerm/releases) 下载最新 **x64 NSIS 安装包**。

安装包会附带开源 [com0com](https://com0com.sourceforge.net/) 虚拟串口驱动，使虚拟 COM 桥可以开箱即用。com0com 是独立第三方 GPL 组件。Windows ARM64 与 MSI 暂不属于当前发布目标。安装包尚未做 Authenticode 签名，因此首次安装可能触发 SmartScreen 提示。

### Linux

从 [GitHub Releases](https://github.com/hamburger-os/TauTerm/releases) 选择 x86_64 的 `.deb`、`.rpm` 或 `.AppImage`。Linux 发行产物以 Ubuntu 22.04 为构建基线。

Linux 虚拟串口桥使用进程内 POSIX PTY，不需要 `socat` 或额外 helper 进程。

### macOS

从 [GitHub Releases](https://github.com/hamburger-os/TauTerm/releases) 下载 **Apple Silicon** 的 `.dmg` / updater app archive。macOS Intel 暂不属于当前发布目标。

macOS 仍属于**技术预览**且尚未 notarize，若被 Gatekeeper 拦截，首次可右键 → 打开。

准确的平台、架构、安装包与签名状态请查看 [支持平台矩阵](docs/SUPPORTED_PLATFORMS.md)。

---

## 安全与可信度

TauTerm 会处理远程凭据、网络流量、串口设备、本地文件和软件更新，因此安全边界是产品设计的一部分，而不是发布前最后补上的功能。

- SSH 主机指纹确认与日志脱敏。
- 凭据存储优先使用 OS Keyring；不可用时可解锁 Argon2id + AES-256-GCM 加密 vault，并仅在当前应用会话中保持解锁。
- SSH 连接表单中输入的密码不会自动保存。
- Windows 下主程序以普通用户权限运行；虚拟端口等特权操作委托给后台服务，开发环境中服务不可用时才使用回退路径。
- Tauri updater 产物由发布流程进行密码学签名与校验。
- 最终发布文件在公开前经过校验，并生成 GitHub build provenance attestation。

发现疑似安全问题时，请按照 [SECURITY.md](SECURITY.md) 通过私密渠道报告。

---

## 路线图

| 目标版本 | 重点 |
|---|---|
| **v0.6** | 本地 Shell、会话分组、SSH 隧道与跳板机 |
| **v0.7** | 网络调试完善、FTP、录制与分屏 |
| **v1.0** | 跨平台发布级验证、性能预算、插件 SDK 文档与 TRDP |
| **v1.1** | 「终端 + 示波器」：WebGL 绘图、FFT、FireWater/JustFloat 兼容 |

路线图版本是目标而不是承诺；优先级会根据真实用户反馈调整。已经发布的内容请以 [CHANGELOG.md](CHANGELOG.md) 与 [GitHub Releases](https://github.com/hamburger-os/TauTerm/releases) 为准。

---

## 架构与贡献

TauTerm 使用**微内核插件架构**。内核提供共享的平台能力，协议实现通过统一的 Adapter / Manifest 模型注册。当前设计详见 [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)。

欢迎参与：

- 发现可复现问题？[提交 Bug](https://github.com/hamburger-os/TauTerm/issues/new/choose)。
- 缺少某个让你无法从现有工具迁移的功能？[提交功能建议](https://github.com/hamburger-os/TauTerm/issues/new/choose)。
- 想贡献代码？阅读 [CONTRIBUTING.md](CONTRIBUTING.md) 与 [docs/BUILDING.md](docs/BUILDING.md)。
- 对协议插件感兴趣？从 [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) 中的插件架构开始。

如果 TauTerm 对你有帮助，给仓库一个 Star 能帮助更多网络和嵌入式工程师发现它。

---

## License

TauTerm 可由用户任选 **MIT License** 或 **Apache License 2.0** 使用。

Windows 安装包包含独立第三方 GPL 组件 [com0com](https://com0com.sourceforge.net/)；它的许可证不受 TauTerm 双许可证影响。

<p align="center"><strong>TauTerm —— 一台终端，服务机房与实验台。</strong></p>
