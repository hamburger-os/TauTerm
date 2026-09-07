<p align="center">
  <img src="src-tauri/icons/icon.png" width="112" alt="TauTerm logo">
</p>

<h1 align="center">TauTerm</h1>

<p align="center"><strong>面向连接系统的本地优先工程工作台。</strong></p>

<p align="center">
  SSH/SFTP · 串口 · 本地 Shell · TCP/UDP · TFTP · Telnet · iPerf · TRDP
</p>

<p align="center">
  <a href="https://github.com/hamburger-os/TauTerm/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/hamburger-os/TauTerm/actions/workflows/ci.yml/badge.svg?branch=master"></a>
  <a href="https://github.com/hamburger-os/TauTerm/releases"><img alt="Release" src="https://img.shields.io/github/v/release/hamburger-os/TauTerm?include_prereleases&label=release"></a>
  <a href="LICENSE"><img alt="License: MIT OR Apache-2.0" src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg"></a>
  <img alt="Rust + Tauri" src="https://img.shields.io/badge/Rust%20%2B%20Tauri-v2-24C8DB">
</p>

<p align="center">
  <a href="https://github.com/hamburger-os/TauTerm/releases"><strong>下载</strong></a>
  · <a href="docs/community/BUILDING.md">构建</a>
  · <a href="CONTRIBUTING.md">参与贡献</a>
  · <a href="README.md">English</a>
</p>

TauTerm 把远端系统、嵌入式设备和网络调试工作流放进同一个桌面工作区。它面向需要在服务器、实验室设备和工业网络之间切换的工程师，让这些上下文共享 Session、日志、自动化和一致的操作方式。

核心工程流程采用 **本地优先** 设计：即使没有账号或云服务，也应能在实验室、工厂和隔离网络中完成主要调试工作。

> 本 README 描述当前 `master` 分支。正式安装包可能滞后于 `master`；版本变化的唯一权威记录见 [CHANGELOG.md](CHANGELOG.md)。

![TauTerm 工作区](docs/assets/hero-zh-CN.webp)

## 为什么是 TauTerm？

- **一个工程工作区** —— 终端、设备、文件、网络与分析工作流使用统一的 Session 模型。
- **远端与嵌入式并列** —— SSH/SFTP、本地 Shell 与串口、TCP/UDP、TFTP、Telnet、iPerf、TRDP 处于同一工作台。
- **本地优先** —— 核心调试能力不依赖云账号。
- **可扩展架构** —— 协议语义留在模块中，Session、Workspace、日志和自动化等公共能力统一复用。
- **跨平台目标** —— Windows、Linux、macOS 均有发布目标，并明确记录平台差异。

## 当前支持

| 工作流 | 当前 `master` |
|---|---|
| SSH 终端 + SFTP + 远端 journald | ✅ |
| 串口 RS-232/485 + Text/HEX/Dual + X/Y/ZModem | ✅ |
| 原生 PTY/ConPTY 本地 Shell | ✅ |
| TCP/UDP 网络调试 | ✅ |
| TFTP 客户端/服务端 | ✅ |
| Telnet 终端 | ✅ |
| iPerf2 / iPerf3 测试 | ✅ |
| TRDP Node + 被动实时/离线 Monitor | ✅ |
| 适用 Session 的 Lua 脚本与自动回复 | ✅ |
| 可持久化的 1–4 分屏 Workspace | ✅ |

TRDP 包含主动 PD/MD Node 与被动 Monitor/抓包分析，但**不声明** SDTv2/SDTv4 安全验证或安全认证能力。

## 安装

请从 [GitHub Releases](https://github.com/hamburger-os/TauTerm/releases) 下载正式构建。

当前发布目标包括 Windows x86_64、Linux x86_64 与 macOS Apple Silicon。安装包类型、签名状态和平台运行时依赖统一维护在 [支持平台](docs/community/SUPPORTED_PLATFORMS.md)。

## 从源码构建

常规开发入口：

```bash
git clone https://github.com/hamburger-os/TauTerm.git
cd TauTerm
npm install
npm run tauri dev
```

需要 Node.js 22、由 `rust-toolchain.toml` 固定的 Rust 工具链，以及对应平台的构建依赖。完整环境说明见 [源码构建指南](docs/community/BUILDING.md)。

## 文档

文档按读者分层，而不是在多个文件里重复维护同一事实：

- **用户与社区开发者：** 本 README、[CONTRIBUTING.md](CONTRIBUTING.md) 和 [docs/community/](docs/community/)。
- **架构/方案审查：** [docs/README.md](docs/README.md) 与 [docs/modules/](docs/modules/)，使用中文供项目维护者审查。
- **维护者开发操作：** [docs/maintainer/DEVELOPMENT.md](docs/maintainer/DEVELOPMENT.md) 保存中文日常开发流程与命令速查。
- **标准与权威知识：** [docs/knowledge/](docs/knowledge/) 索引实现协议/平台能力时应核对的原始标准和上游官方资料。
- **AI 编码代理：** [AGENTS.md](AGENTS.md) 与 [`.agents/skills/`](.agents/skills/)。
- **产品方向：** [docs/product/](docs/product/) 描述未来方向，不代表已经交付。
- **版本历史：** [CHANGELOG.md](CHANGELOG.md) 是唯一权威来源。

## 参与贡献

欢迎贡献。请先阅读 [CONTRIBUTING.md](CONTRIBUTING.md)，再按照 [源码构建指南](docs/community/BUILDING.md) 配置环境。

如果修改架构或行为，Pull Request 必须同步更新对应设计文档；CI 会执行文档一致性检查。

## 安全

安全问题请按照 [SECURITY.md](SECURITY.md) 中的流程报告。请勿在公开 Issue 中提交凭据、私钥、敏感端点或敏感日志。

## 许可证

TauTerm 源代码采用 **MIT OR Apache-2.0** 双许可证，可任选其一。详见 [LICENSE](LICENSE) 与 [LICENSE-APACHE](LICENSE-APACHE)。

随项目分发或 vendoring 的第三方组件保留各自许可证，清单见 [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md)。
