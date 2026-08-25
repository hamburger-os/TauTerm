# 构建与运行指南

> 终端用户直接安装请见 [README.md](../README.md) 的下载章节。本文档面向需要从源码构建的开发者。

## 环境要求

| 组件 | 版本要求 | 说明 |
|------|---------|------|
| **Node.js** | >= 18 | 前端运行时与包管理器 |
| **Rust** | >= 1.96 (推荐) / >= 1.75 (最低) | 后端编译工具链 |
| **npm** | >= 9 | 随 Node.js 附带 |
| **NSIS** | >= 3.0 | Windows 安装包构建工具（仅 Windows 构建需要） |

> **注意**: Rust 1.96+ 内置 `rust-lld` 链接器，在 Windows 上**无需额外安装 Visual Studio Build Tools**。如果使用较低版本 Rust，则需要安装 VS Build Tools 提供 MSVC 链接器。

---

## Windows 环境安装

### 1. 安装 Node.js

从 [nodejs.org](https://nodejs.org/) 下载 LTS 版本安装，或使用 winget：

```powershell
winget install --id OpenJS.NodeJS.LTS --source winget
```

验证安装：

```powershell
node --version   # 应输出 >= v18.0.0
npm --version    # 应输出 >= 9.0.0
```

### 2. 安装 Rust

使用 winget 安装 rustup（Rust 官方工具链管理器）：

```powershell
winget install --id Rustlang.Rustup --source winget
```

安装完成后，**重新打开终端**使环境变量生效，然后设置默认工具链：

```powershell
rustup default stable
```

> **下载慢？** 可设置国内镜像源加速：
> ```powershell
> $env:RUSTUP_DIST_SERVER = "https://mirrors.ustc.edu.cn/rust-static"
> rustup default stable
> ```

验证安装：

```powershell
rustc --version   # 应输出 >= rustc 1.75.0
cargo --version   # 应输出 >= cargo 1.75.0
```

### 3. 链接器

**使用 Rust 内置的 rust-lld**

Rust 1.96+ 自带 LLVM 链接器 `rust-lld`，无需额外安装，编译器会自动使用。

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
  libkeyring-dev

# 安装 Node.js（使用 NodeSource）
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

# 安装 Node.js（使用 Homebrew）
brew install node

# 安装 Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
```

---

## 环境验证

运行以下命令确认所有组件正确安装：

```powershell
# Windows (PowerShell)
node --version   # >= 18
npm --version    # >= 9
rustc --version  # >= 1.75
cargo --version  # >= 1.75

# Linux / macOS
node --version && npm --version && rustc --version && cargo --version
```

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
2. `tsc && vite build` — 前端 TypeScript 编译 + Vite 打包
3. `build.rs` — 根据目标架构（x86_64 → x64, i686 → x86）将 7 个驱动文件复制到打包根目录；同时为服务二进制创建占位文件（满足 tauri-build 对资源路径的校验）
4. `cargo build --release` — Rust 后端编译（产出主程序 `tauterm.exe` 与特权服务 `tauterm-service.exe`）
5. `prepare-service-bin.js` — 将 `target/release/tauterm-service.exe` 复制到 `src-tauri/binaries/`（打包资源）
6. **NSIS 打包** — 生成安装程序，内含 com0com 驱动文件 + 服务二进制 + post-install hook（安装时自动执行 `setupc.exe install` 并注册 `TauTermService`）

> **平台差异**：服务二进制资源（`binaries/tauterm-service.exe`）仅声明在 `tauri.windows.conf.json` 的 `bundle.resources` 中（与基础 `tauri.conf.json` 合并），Linux/macOS 构建不会引用该文件，`build.rs` 的占位文件也只在 Windows 创建，因此非 Windows 平台可正常构建。

**构建产物**：

```
src-tauri/target/release/bundle/nsis/
├── TauTerm_<version>_x64-setup.exe    # NSIS 安装程序（推荐分发）
└── TauTerm_<version>_x64_en-US.msi    # WiX MSI 安装包
```

**安装程序行为**：

- **安装时**：NSIS post-install hook 自动执行 `setupc.exe install` 安装 com0com 内核驱动（安装程序天然以管理员身份运行），并注册/启动 `TauTermService`（LocalSystem 特权服务，负责虚拟端口对的创建/删除/清理）
- **卸载时**：NSIS pre-uninstall hook 停止并删除 `TauTermService`，再执行 `setupc.exe uninstall` 移除驱动
- **运行时权限**：主程序 `tauterm.exe` 以普通用户权限（asInvoker）运行——驱动安装、端口对创建/删除等特权操作通过命名管道委托给 `TauTermService`，不弹 UAC
- **运行时回退**：如果服务不可用（如开发模式未注册服务），应用回退到按需 UAC 路径（`ShellExecuteEx("runas")` 执行 setupc 序列）

> **注意**：必须使用 `npm run tauri:build`（等效于 `npx tauri build --bundles nsis`）来生成安装程序。不加 `--bundles nsis` 时 Tauri v2 可能静默跳过 NSIS 打包，只生成 `tauterm.exe`。

### Linux / macOS

```bash
npm run tauri build
```

构建产物位于 `src-tauri/target/release/bundle/`：`.deb` / `.rpm` / `.AppImage`（Linux），`.dmg` / `.app`（macOS）。

---

## 使用虚拟串口桥接

> **驱动版本**：使用 com0com v3.0.0.0（GPL 开源内核驱动），支持 Windows 10/11 x64/x86。详细的 com0com 使用与故障排查请参考 [tauterm-com0com skill](../.agents/skills/tauterm-com0com/SKILL.md)。

虚拟串口功能**默认开启**，连接串口时自动创建 COM 端口对。基本使用流程：

1. **连接串口**：在连接对话框中，串口配置区域的"启用虚拟串口"开关默认开启，"设备数量"（1-4）决定创建多少对端口
2. **查看端口对**：连接成功后，状态栏显示 `VPort: COM22↔COM23, …`，端口 A（COM22）由 TauTerm 占用，端口 B（COM23）供外部工具打开
3. **外部工具读取**：用任意串口工具（如 SSCOM、Putty、Python `pyserial`）打开端口 B（COM23），即可实时接收物理串口的数据
4. **外部工具写入**：外部工具向端口 B 写入的数据会自动转发到物理串口，实现双向桥接
5. **断开自动清理**：断开串口或关闭 TauTerm 时，自动删除所有虚拟端口对
6. **手动清理残留**：状态栏右侧常驻 `[清理残留端口]` 按钮，点击可触发 UAC 提权批量清理所有已知残留端口对

> **注意**：
> - 首次使用需安装 com0com 内核驱动 — 安装 TauTerm 时由 NSIS 安装程序自动完成；若驱动被意外卸载，状态栏会显示 `VPort 未就绪 — 驱动未安装` 并提供 `[修复]` 按钮（服务模式下由服务安装，无 UAC；回退模式下触发 UAC 提权安装）
> - **开发模式**：`npm run tauri dev` 启动的应用通常没有注册服务，虚拟端口操作走 UAC 回退路径。点击状态栏 `[清理残留端口]` 手动触发清理，或下次连接时自动由 UAC 批量清理
