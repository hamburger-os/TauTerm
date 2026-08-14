# TauTerm

> **快速、现代、跨平台的全功能终端模拟器 —— 为网络工程师与嵌入式开发者而生。**

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE)
[![Release](https://img.shields.io/badge/release-v0.4.0-brightgreen.svg)](https://github.com/hamburger-os/TauTerm/releases)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](#)
[![Framework](https://img.shields.io/badge/Tauri-v2-67D6F8.svg)](https://tauri.app)

基于 **Tauri v2**（Rust + React + TypeScript）构建，TauTerm 将现代的原生级界面体验与微内核插件架构合二为一：内核不包含任何协议实现——每种会话类型（串口、SSH、Telnet、TFTP、FTP、iPerf……）都是独立插件。

**This document is the Chinese mirror of the [English README](README.md).**

<!-- TODO(screenshots): 主界面截图占位 -->

---

## 为什么选择 TauTerm？

如果你的一天在终端里度过——SSH 连生产服务器、串口烧录固件，或者两者都要——TauTerm 正是为你打造的：

- **免费开源** —— MIT / Apache-2.0 双授权，持续活跃维护
- **轻量原生级体验** —— Tauri v2（Rust + WebView2），安装包仅 8.0 MB
- **双工作流一应用** —— 运维侧 SSH/SFTP/TFTP/iPerf，嵌入式侧串口/YModem/Modbus/Lua
- **插件架构天生可扩展** —— 微内核设计，每种协议都是独立插件
- **安全优先默认值** —— 系统原生 keyring、主机指纹确认、日志脱敏
- **现代设计系统** —— Liquid Glass v3、三套主题、流畅动画

---

## 功能特性

### 🖥️ 面向网络工程师

- **SSH + SFTP 单连接** —— 终端与文件传输在同一条会话上复用（SideChannel 架构，无需二次登录）
- **SFTP 文件管理器** —— 远端目录浏览、上传/下载、批量删除、重命名、属性查看、breadcrumb 路径跳转
- **TFTP 服务器/客户端** —— RRQ/WRQ 服务端监听、GET/PUT 客户端、RFC 7440 滑动窗口、CRC32 校验、可调超时、指数退避重传
- **Telnet** —— RFC 854 选项协商（ECHO/SGA/BINARY/NAWS）、本地回显自适应、窗口尺寸实时同步、keepalive 保活
- **iPerf 网络测速** —— iperf2（自研 wire-compatible 实现）+ iperf3（vendored riperf3）；TCP/UDP、`-t/-b/-P/-i/-w`、双向模式、实时速率曲线与历史记录
- **远端 journald 查看器** —— SSH 流式追踪与历史查询、级别/关键字/单元过滤、游标分页、JSON 导出（进度显示/可取消）
- *规划中（v0.6–v0.7）：SSH 隧道与端口转发 UI、跳板机、会话树分组、本地 Shell、FTP、会话录制*

### 🔌 面向嵌入式开发者

- **串口（RS-232/485）+ 虚拟串口桥接** —— 连接时自动创建端口对（com0com），外部工具可实时旁路物理链路；孤儿端口自动清理
- **XModem / YModem / ZModem 传输** —— 按会话选择协议，从活动串口 inline 移交所有权
- **会话级字符编码转码** —— UTF-8、GBK、GB18030、Big5、Shift-JIS、EUC-JP、EUC-KR、ISO-8859-1；接收流式解码、发送方向自动转码、Dual 模式按编码解码、日志恒为可读 UTF-8
- **Dual 双模显示** —— 可拖拽分栏并排 ASCII 文本 + HEX，毫秒级时间戳、`\r\n`/`\n`/`\r` 自动分帧、TX/RX 颜色区分
- **四模式发送栏** —— 基础发送（文本/HEX、换行符、循环、历史）、指令面板（预定义序列、拖拽排序、循环执行）、自动应答（可视化规则、5 种匹配模式、10 种动态宏、定时触发）、脚本编辑器
- **嵌入式 Lua 5.4 脚本** —— 每会话独立 VM 沙箱隔离；`send()` 原始字节透传、`send_text()` 按会话编码转码（GBK 设备发送中文不乱码）
- **嵌入式开发工具箱** —— CRC8/16/32 多预设、Base64/HEX/浮点/大小端转换、位操作、C `sizeof` 计算器、Modbus RTU/ASCII 与 AT 指令解析器
- *规划中（v1.1）：波形绘图引擎 + FFT，兼容 FireWater/JustFloat 协议——现有固件数据格式开箱即用*

### ⚡ 面向所有人

- **Liquid Glass v3 设计系统** —— 三套主题（Google Glow / Obsidian / Frosted），Framer Motion 动画
- **统一标签页会话** —— 串口、SSH、FTP、iPerf 共享同一标签栏；离线会话配置、右键一键重连
- **凭据存储** —— 系统原生 keyring（Windows 凭据管理器 / macOS 钥匙串 / Secret Service）+ AES-256-GCM 文件降级
- **安全优先默认值** —— SSH 首次连接指纹确认、密钥变更警告、日志脱敏（密码/密钥/Token）、Agent 转发默认禁用
- **自动更新** —— Tauri updater，可配置检查频率，一键下载安装并重启
- **命令面板、终端搜索、全键位自定义** —— `Ctrl+Shift+P` 面板、`Ctrl+F` 搜索 buffer、点击录制式改键
- **会话数据日志** —— text/hex/dual 三种格式、自动分卷与过期清理、一键启停、状态栏实时指示
- **国际化** —— 中文 / 英文，插件命名空间隔离，运行时切换

---

## 性能

轻量是我们的首要目标之一——每个版本都按性能预算验收（冷启动、空闲内存、滚动吞吐）。

| 指标 | v0.4.0（实测） |
|---|---|
| Windows 安装包（`x64-setup.exe`，含 com0com 驱动） | **8.0 MB** |
| 应用二进制 | **25.7 MB** |
| 冷启动与空闲内存 | 随 v0.8 性能验收里程碑发布基准测试 |

*测试环境 Windows 11 x64。安装包内含 com0com 虚拟串口驱动。*

---

## 快速安装

### Windows

从 [GitHub Releases](https://github.com/hamburger-os/TauTerm/releases) 下载最新安装包（`TauTerm_x.x.x_x64-setup.exe`）。

安装包内附带并自动安装开源的 [com0com](https://com0com.sourceforge.net/) 虚拟串口驱动（GPL v3.0.0.0，源码可获取），虚拟串口桥开箱即用；卸载 TauTerm 时会一并移除驱动。

### macOS / Linux

预编译安装包正在准备中。当前请从源码构建——见 [docs/BUILDING.md](docs/BUILDING.md)。

---

## 协议支持矩阵

| 协议 | 状态 | 内容类型 | 传输支持 | I/O 模式 |
|---|---|---|---|---|
| **Serial**（RS-232/485） | ✅ 已实现 | terminal | YModem / XModem / ZModem（Inline） | Sync |
| **SSH** | ✅ 已实现 | terminal | SFTP（SideChannel） | Async |
| **TFTP** | ✅ 已实现 | custom | 独立 UDP 传输引擎 | Headless |
| **Telnet** | ✅ 已实现 | terminal | — | Sync |
| **iPerf2 / iPerf3** | ✅ 已实现 | custom | 独立测速引擎 | Async |
| **TCP Raw** | 📋 v0.6 | terminal | — | Async |
| **Shell Local**（PTY） | 📋 v0.6 | terminal | — | Sync |
| **FTP** | 📋 v0.7 | file_browser | FTP（SeparateConnection） | Async |
| **TRDP** | 📋 v0.7 | terminal | — | Async |
| **NFS** | 🔮 远期 | file_browser | NFS（SeparateConnection） | Async |
| **UDP Monitor** | 🔮 远期 | stats_dashboard | — | Async |

---

## 发展路线图

```
v0.5  凭据&安全收尾：keyring 持久化、日志脱敏接入生产管线
v0.6  运维可用性：Shell Local（PTY）、会话树分组管理、
      SSH 隧道 UI + 跳板机、Agent Forwarding、TCP Raw、
      asInvoker 权限降级 + UAC 提权助手
v0.7  运维增强+嵌入式收尾：FTP、会话录制、终端分屏、TRDP
v1.0  正式版：Windows 完整验证、macOS/Linux 核心可用、
      性能指标达标、插件 SDK 文档
v1.1  波形引擎 ——「终端+示波器」：WebGL 绘图、FFT、
      兼容 FireWater/JustFloat
v1.x  远期（按优先级）：多窗口、动态插件加载、
      插件市场、NFS、云端会话同步
```

---

## 开发

- **[docs/BUILDING.md](docs/BUILDING.md)** —— 三平台环境配置、开发模式、生产构建
- **[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)** —— 微内核设计、插件架构、I/O 策略、安全模型
- **[CONTRIBUTING.md](CONTRIBUTING.md)** —— 贡献指南

## 许可证

TauTerm 按以下任一许可证授权：

- [MIT License](LICENSE)
- [Apache License, Version 2.0](LICENSE-APACHE)

任选其一。

Windows 安装包随附的 [com0com](https://com0com.sourceforge.net/) 内核驱动是独立的第三方 GPL 组件，其许可证不受 TauTerm 双授权影响。

---

**TauTerm** —— 一台终端，服务机房与实验室。
