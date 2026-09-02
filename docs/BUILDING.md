# 构建与运行指南

> 终端用户直接安装请见 [README.md](../README.md) 的下载章节。本文档面向需要从源码构建的开发者。

## 环境要求

| 组件 | 版本要求 | 说明 |
|------|---------|------|
| **Node.js** | 22.x | 前端运行时与包管理器（CI 与发布工作流固定使用 Node 22） |
| **Rust** | 仓库锁定版本 | 由根目录 `rust-toolchain.toml` 精确锁定稳定版，并声明 clippy 与 rustfmt |
| **npm** | 随 Node.js 22 附带 | 依赖安装与脚本运行 |
| **NSIS** | >= 3.0 | Windows 安装包构建工具（仅 Windows 构建需要） |

> **注意**：Rust 的精确版本只以 `rust-toolchain.toml` 为准。请通过 rustup 进入仓库后运行 Rust 命令，不要在本机另行选择其他版本。Windows 构建仍需要可用的 MSVC/Windows SDK 环境；Rust 工具链本身不替代这些系统组件。

---

## Windows 环境安装

### 1. 安装 Node.js

从 [nodejs.org](https://nodejs.org/) 安装 Node.js 22，或使用 winget：

```powershell
winget install --id OpenJS.NodeJS --source winget
```

验证安装：

```powershell
node --version   # 应输出 v22.x
npm --version
```

### 2. 安装 Rust

使用 winget 安装 rustup（Rust 官方工具链管理器）：

```powershell
winget install --id Rustlang.Rustup --source winget
```

安装完成后，**重新打开终端**使环境变量生效。进入仓库并首次运行 Rust 命令时，rustup 会按照根目录的 `rust-toolchain.toml` 安装或选择精确工具链及其组件：

```powershell
cd C:\path\to\TauTerm
rustup show active-toolchain
cargo --version
```

> **下载慢？** 可设置国内镜像源加速：
> ```powershell
> $env:RUSTUP_DIST_SERVER = "https://mirrors.ustc.edu.cn/rust-static"
> rustup show active-toolchain
> ```

验证安装：

```powershell
rustup show active-toolchain   # 应与 rust-toolchain.toml 一致
rustc --version
cargo --version
npm run toolchain:check
```

### 3. 链接器

**工具链组件**

`rust-toolchain.toml` 声明仓库所需的 `rustfmt` 与 `clippy` 组件；具体链接器与 Windows SDK/MSVC 环境取决于目标平台。

---

## Linux 环境安装

### Ubuntu / Debian

```bash
# 安装系统依赖
sudo apt update
sudo apt install -y \
  libwebkit2gtk-4.1-dev \
  libappindicator3-dev \
  librsvg2-dev \
  patchelf \
  libssl-dev \
  libgtk-3-dev \
  libsoup-3.0-dev \
  libjavascriptcoregtk-4.1-dev \
  libudev-dev

# 安装 Node.js 22（使用 NodeSource）
curl -fsSL https://deb.nodesource.com/setup_22.x | sudo -E bash -
sudo apt install -y nodejs

# 安装 Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
```

### Fedora / RHEL

```bash
sudo dnf install -y \
  webkit2gtk4.1-devel \
  libappindicator-gtk3-devel \
  librsvg2-devel \
  patchelf \
  openssl-devel

# Node.js 和 Rust 安装同上
```

### Arch Linux

```bash
sudo pacman -S --needed \
  webkit2gtk-4.1 \
  libappindicator-gtk3 \
  librsvg \
  patchelf \
  openssl
```

---

## macOS 环境安装

```bash
# 安装 Xcode Command Line Tools
xcode-select --install

# 安装 Node.js 22（使用 Homebrew）
brew install node@22

# 安装 Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"

# Linux/macOS 虚拟串口桥接由 TauTerm 进程内创建 POSIX PTY，无需额外 helper
```

---

## 环境验证

运行以下命令确认所有组件正确安装：

```powershell
# Windows (PowerShell)
node --version   # v22.x
npm --version
rustup show active-toolchain
rustc --version
cargo --version
npm run toolchain:check

# Linux / macOS
node --version && npm --version && rustc --version && cargo --version
```

---

## 常用命令

`package.json` 的 `scripts` 是 npm 命令的实现来源；下表说明开发者与维护者会直接使用的入口。

| 命令 | 用途 |
|------|------|
| `npm run tauri dev` | 启动完整 Tauri 开发环境（Vite + Rust 后端 + 桌面窗口） |
| `npm run dev` | 仅启动 Vite 前端开发服务器 |
| `npm run build` | 执行 TypeScript 检查并构建前端生产资源 |
| `npm run preview` | 本地预览已经构建的前端资源 |
| `npm run tauri:build` | Windows 使用开发打包配置生成 NSIS 安装包 |
| `npm run tauri -- build` | 使用当前平台的标准 Tauri 配置构建安装包 |
| `npm run build:release` | 更新并固定当前 stable Rust，运行完整质量检查，再构建当前平台的正式产物 |
| `npm run toolchain:check` | 验证 `rust-toolchain.toml` 使用精确稳定版本并包含 clippy/rustfmt |
| `npm run version:check` | 检查各处应用版本元数据是否一致 |
| `npm run release:check -- X.Y.Z` | 检查指定发布版本、CHANGELOG 与 Release Notes |
| `npm run version:sync` | 将 `package.json` 的版本同步到 Cargo 与 Tauri 配置 |
| `npm run check-com0com` | 校验打包所需的 com0com 驱动文件 |
| `npm run check-reserved-region` | 校验虚拟串口测试与产品保留区配置一致 |
| `npm run check:icons` | 校验图标资产集合与基本视觉约束；追加 `-- --strict` 可将 advisory warning 视为失败 |
| `npm run check:session-buffer` | 回归验证终端挂载前到达的启动数据按顺序保留且受内存上限约束 |
| `npm run prompt:icon -- <key>` | 从语义注册表生成不可删减的图标提示词，并列出必须附带的三张家族参考图；`--json` 输出结构化结果 |
| `npm run preview:icons` | 生成图标资产预览页 |

`postversion` 是 npm 生命周期钩子，不需要直接执行；运行 `npm version ...` 时会自动调用版本同步脚本。完整发布顺序见 [RELEASING.md](RELEASING.md)。

---

## Windows 管理员 Shell 构建说明

管理员 Local Shell 不增加独立 sidecar，也不使用 `TauTermService`。打包后的同一个 `tauterm.exe` 会在 Tauri 初始化前识别内部 helper 参数；用户选择“新建(以管理员身份)”时，普通权限主进程通过 Windows `runas` 启动该一次性 helper，由 helper 创建管理员 ConPTY，并通过一对随机本地逻辑单向命名管道分别传输命令和事件。server 句柄需要 duplex-capable 权限，以便连接后把非阻塞等待模式切回阻塞模式；helper 端仍只打开相应的读端或写端。因而标准 Tauri 开发构建和正式安装包会自动包含此能力，不需要额外复制文件或注册服务。

该路径只能在 Windows 上进行完整人工冒烟验证：启动 Local Shell，右键父卡片选择“新建(以管理员身份)”，接受 UAC 后确认子卡片出现盾牌，并执行 `net session` 等需要管理员权限的只读命令；再验证拒绝 UAC 时不会创建子卡片。WSL 会话不显示该菜单。

## 图标资产生成与验收

图标不能直接根据自由文本生成并覆盖 `src/assets/icons/`。先在 `src/assets/icons/prompts.md` 注册语义，再运行：

```powershell
npm run prompt:icon -- <key>
```

生成时必须逐字使用输出提示词，并附带输出列出的三张固定家族参考图。候选图先保存在运行时图标目录之外并规范化为 256×256 RGBA；先执行候选检查，通过后才能替换正式资产：

```powershell
npm run check:icons -- --strict --candidate <key> <png-path>
npm run check:icons -- --strict
npm run preview:icons
```

第一项校验注册表、RGBA/透明边距、alpha 核心以及浅冰蓝调色板漂移；第二项生成深浅背景的 12/14/18/24px 对照板，并固定显示普通功能族和微型控制族参考图。机器检查不能替代语义与小尺寸人工验收。家族锚点、分类和调色板阈值集中在 `src/assets/icons/style-contract.json`，不要在生成任务中临时覆盖。

---

## 开发模式

```bash
# 克隆仓库
git clone https://github.com/hamburger-os/TauTerm.git
cd TauTerm

# 安装前端依赖
npm install

# 启动开发模式（同时启动 Vite 开发服务器和 Tauri 桌面窗口）
npm run tauri dev
```

- Vite 开发服务器运行在 `http://localhost:5173`
- Tauri 窗口自动打开，支持热更新（前端）和热重载（Rust）

> **首次运行**：`npm run tauri dev` 会编译所有 Rust 依赖，需要 **5-15 分钟**（视网络和 CPU 而定）。后续编译将利用缓存，通常只需几秒。

---

## 生产构建

### Windows（生成 .exe 安装包）

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
2. `check-reserved-region.js` — 校验 `scripts/test-serial-session.py` 与 `src-tauri/src/virtual_port/manager.rs` 的 com0com 预留端口/bus 段常量一致，避免测试脚本与产品的虚拟串口互占/互删
3. `tsc && vite build` — 前端 TypeScript 编译 + Vite 打包
4. `build.rs` — Windows 下按目标架构（x86_64 → x64，i686 → x86）复制 7 个驱动文件，并创建服务资源占位文件供 `tauri-build` 校验；非 Windows 不创建服务占位文件
5. `cargo build --release` — Rust 后端编译，产出主程序 `tauterm.exe` 与 Windows 专用 `tauterm-service.exe`
6. `prepare-service-bin.js` — Windows 打包前将服务二进制复制到 `src-tauri/binaries/`；非 Windows 跳过
7. **NSIS 打包** — 生成 x64 安装程序，内含 com0com 驱动、服务二进制与 NSIS hooks；安装时执行驱动安装并注册 `TauTermService`

> **平台差异**：服务二进制资源（`binaries/tauterm-service.exe`）仅声明在 `tauri.windows.conf.json` 的 `bundle.resources` 中（与基础 `tauri.conf.json` 合并），Linux/macOS 构建不会引用该文件，`build.rs` 的占位文件也只在 Windows 创建，因此非 Windows 平台可正常构建。

**构建产物**：

```
src-tauri/target/release/bundle/nsis/
└── TauTerm_<version>_x64-setup.exe    # NSIS 安装程序（当前 Windows 发布产物）
```

**安装程序行为**：

- **安装时**：NSIS post-install hook 自动执行 `setupc.exe install` 安装 com0com 内核驱动（安装程序天然以管理员身份运行），并注册/启动 `TauTermService`（LocalSystem 特权服务，负责虚拟端口对的创建/删除/清理）
- **卸载时**：NSIS pre-uninstall hook 停止并删除 `TauTermService`，再执行 `setupc.exe uninstall` 移除驱动
- **运行时权限**：主程序 `tauterm.exe` 以普通用户权限（asInvoker）运行——驱动安装、端口对创建/删除等特权操作通过命名管道委托给 `TauTermService`，不弹 UAC
- **运行时回退**：如果服务不可用（如开发模式未注册服务），应用回退到按需 UAC 路径（`ShellExecuteEx("runas")` 执行 setupc 序列）

> **注意**：必须使用 `npm run tauri:build`（等效于 `npx tauri build --bundles nsis`）来生成安装程序。不加 `--bundles nsis` 时 Tauri v2 可能静默跳过 NSIS 打包，只生成 `tauterm.exe`。

### Linux / macOS

```bash
npm run tauri -- build
```

构建产物位于 `src-tauri/target/release/bundle/`：`.deb` / `.rpm` / `.AppImage`（Linux），`.dmg` / `.app`（macOS）。

---

## 使用虚拟串口桥接

> **驱动版本**：Windows 使用 com0com v3.0.0.0（GPL 开源内核驱动），支持 Windows 10/11 x64/x86。详细的 com0com 使用与故障排查请参考 [tauterm-com0com skill](../.agents/skills/tauterm-com0com/SKILL.md)。

> **非 Windows 平台**：Linux/macOS 使用 `src-tauri/src/virtual_port/pty.rs` 中的进程内 POSIX PTY 后端创建端点。无需 `socat`、Homebrew helper、shell `PATH` 配置、`/tmp` 符号链接或外部 helper 进程。该桥接传输字节流，不模拟硬件 UART 的波特率、调制解调器控制线或电气行为。

虚拟串口功能**默认开启**，连接串口时自动创建端口对。基本使用流程：

1. **连接串口**：在连接对话框中，串口配置区域的"启用虚拟串口"开关默认开启，"设备数量"（1-4）决定创建多少对端口
2. **查看端口对**：连接成功后，状态栏显示 `VPort: COM22↔COM23, …`，端口 A（COM22）由 TauTerm 占用，端口 B（COM23）供外部工具打开
3. **外部工具读取**：用任意串口工具（如 SSCOM、Putty、Python `pyserial`）打开端口 B（COM23），即可实时接收物理串口的数据
4. **外部工具写入**：外部工具向端口 B 写入的数据会自动转发到物理串口，实现双向桥接
5. **断开自动清理**：断开串口或关闭 TauTerm 时，自动删除所有虚拟端口对
6. **手动清理残留**：状态栏右侧常驻 `[清理残留端口]` 按钮，点击可触发服务或 UAC 回退路径批量清理已知残留端口对

> **注意**：
> - 首次使用需安装 com0com 内核驱动 — 安装 TauTerm 时由 NSIS 安装程序自动完成；若驱动被意外卸载，状态栏会显示 `VPort 未就绪 — 驱动未安装` 并提供 `[修复]` 按钮（服务模式下由服务安装，无 UAC；回退模式下触发 UAC 提权安装）
> - **开发/便携模式**：如果未注册 `TauTermService`，虚拟端口操作走按需 UAC 回退路径。生产 NSIS 安装包通常通过 Windows 服务完成这些特权操作，主程序本身保持普通用户权限
