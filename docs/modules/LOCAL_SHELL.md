# Local Shell 设计

## 目标

Local Shell 让本机命令行与远端/设备 Session 使用同一套终端工作区，同时保持本地进程、工作目录和权限语义清晰。

## 当前方案

Shell 预设发现属于配置辅助能力，只在进入 Local Shell 配置页时按需执行。自动发现结果以实际探测到的组件名称为主体；仅在有助于区分来源时保留必要分类，例如 WSL 发行版显示为 `WSL · <注册发行版名称>`，不再生成编号或泛化别名。Windows 上的 WSL 发行版探测运行在后台 blocking worker，并设置有限等待时间；平台命令异常或 WSL 服务不可用时只跳过对应预设，不能阻塞配置页面或 Session 核心。

Local Shell 通过平台原生 PTY 创建终端：

- Windows 使用 ConPTY；
- Linux/macOS 使用 Unix PTY。

一个保存配置可以作为父 Session 创建多个独立本地终端。公共核心负责 child lifecycle，Local Shell 工厂负责解析 shell、参数、工作目录并创建对应进程。

Windows 的“以管理员身份新建”只提升某一个 child terminal。主 TauTerm 进程保持普通权限，通过一次性受控 helper/本地 IPC 建立管理员 ConPTY；拒绝 UAC 不产生 child。WSL 仍按 Linux 环境语义运行，不使用该 Windows 原生提权路径。

## 关键数据流

```mermaid
flowchart TB
  Config["Shell 配置"] --> Factory["Channel Factory"]
  Factory --> Normal["普通 child PTY"]
  Factory --> Elevated["Windows 管理员 child"]
  Elevated --> Helper["一次性提权 helper"]
  Normal --> Core["公共 Session 生命周期"]
  Elevated --> Core
```

## 设计边界

- 主应用不因管理员 Shell 整体提权。
- 管理员属性属于单个运行时 child，不写回父配置。
- 每个 child 是独立进程/PTY，不共享 shell 进程状态。
- Local Shell 配置包含“环境变量”字段（KEY/VALUE 行），在表单中位于启动参数之前：后端校验名称（A-Z/a-z/0-9/_）、数量与长度上限后通过 `CommandBuilder` 注入 shell 进程环境，不经过 shell 语法解析；连接前忽略空行，非法/重名名称在表单内联提示，切到 WSL preset 时表单清空该区块；WSL shell 明确拒绝该字段（子系统自己管理环境），管理员 shell 通过 helper 配置一并传递，后端可识别的失败文案由插件层本地化。
- Windows 普通本地 Shell 不经过任何额外的 helper/launcher 进程：`LocalShellDriver` 在创建伪控制台前先锁定随包分发的 ConPTY 引擎（`conpty.dll` + `OpenConsole.exe`，Microsoft Windows Console ConPTY 1.25.260930003，MIT，见 `THIRD_PARTY_LICENSES.md` 与 `src-tauri/vendor/conpty/`），然后把请求的 shell 直接作为 PTY 子进程启动。管理员 Shell 仍只多一次提权 helper，helper 内同样由驱动直接启动 shell，不改变引擎选择。选择该引擎的原因：系统内置 conhost 引擎只有在控制台输入模式已带 `ENABLE_VIRTUAL_TERMINAL_INPUT` 时才转发子进程写入的鼠标上报模式（`DECSET 1000/1002/1003/1006`），而 VT-first 全屏程序（pi 等 Node TUI）在进入 raw mode 之前就写这些序列，导致外层终端永远收不到鼠标；native console 程序（Codex/crossterm）走 `SetConsoleMode` 另一条路径，所以一直正常。随包引擎是 Windows Terminal 使用的实现，对两类程序都按字节转发，因此终端能力由终端引擎负责，不需要按程序白名单或轮询控制台模式。
- 引擎启动握手由驱动在本地完成：引擎初始化伪控制台时会查询光标位置（`ESC[6n`）和设备属性（`ESC[c`），驱动在启动窗口内本地应答并从可见输出中移除这些查询，避免 shell 启动依赖 WebView/xterm 是否就绪，也避免 xterm 的第二次应答变成 shell 的杂散输入。
- Local Shell 使用终端搜索、尺寸与日志能力，但不启用远端传输或全局 SendBar。
- shell 发现只提供可解释的候选；自定义 executable、参数和工作目录必须保持独立字段，避免命令字符串拼接。

## 代码锚点

- `src/plugins/local-shell/`
- `src-tauri/src/plugins/local_shell/`
- `src-tauri/src/kernel/session_store.rs`
- Windows helper 与启动边界位于 `src-tauri/src/`

## 何时更新本文

修改 PTY 后端、shell 解析/发现、父子会话、管理员 child、IPC/权限边界时，必须同步更新本文。
