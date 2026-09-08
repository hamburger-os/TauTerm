# 终端、串口与自动化权威知识索引

## 终端控制序列

### 权威来源

- ECMA-48 — Control Functions for Coded Character Sets: https://ecma-international.org/publications-and-standards/standards/ecma-48/
- xterm.js 官方文档: https://xtermjs.org/docs/
- xterm.js 支持的控制序列: https://xtermjs.org/docs/api/vtfeatures/
- xterm.js Security: https://xtermjs.org/docs/guides/security/

TauTerm 使用 xterm.js 作为终端呈现层。实现终端输入/输出、清屏、光标、颜色或控制序列行为时，不应凭“常见终端习惯”猜测；先确认 ECMA-48 与 xterm.js 实际支持范围。

## 终端剪贴板与快捷键

### 权威来源

- xterm.js Terminal API（`paste()`、`attachCustomKeyEventHandler()`）: https://xtermjs.org/docs/api/terminal/classes/terminal/
- GNOME Terminal 复制/粘贴快捷键说明: https://help.gnome.org/users/gnome-terminal/stable/txt-copy-paste.html.en
- Windows Terminal actions / copy / paste: https://learn.microsoft.com/windows/terminal/customize-settings/actions

复制/粘贴不是 ECMA-48 控制序列本身，而是终端宿主与桌面平台的交互合同。实现时必须区分应用快捷键与发送到 PTY 的控制键；尤其不能把 `Ctrl+C` 简单当作普通复制键，因为它在终端会话中承担中断输入语义。TauTerm 的粘贴路径应使用 xterm.js 提供的 `paste()`，宿主负责快捷键、剪贴板权限、风险确认和焦点恢复。

内部设计：[UI_FOUNDATION.md](../modules/UI_FOUNDATION.md)。

## POSIX PTY

### 权威来源

- POSIX / The Open Group pseudo-terminal interfaces: https://pubs.opengroup.org/onlinepubs/9799919799/

Linux/macOS Local Shell 与虚拟串口 PTY 行为应按目标平台的 POSIX/Unix API 与实际系统限制核对。PTY 是字节/终端会话抽象，不等价于真实 UART 电气层。

## Windows Pseudoconsole / ConPTY

### 权威来源

- Windows Console definitions: https://learn.microsoft.com/windows/console/definitions
- Creating a Pseudoconsole session: https://learn.microsoft.com/windows/console/creating-a-pseudoconsole-session
- CreatePseudoConsole: https://learn.microsoft.com/windows/console/createpseudoconsole

Microsoft 文档明确要求宿主负责输入输出通道，ConPTY 输出是 UTF-8 文本与 Virtual Terminal Sequences 的组合。涉及阻塞管道、双向 I/O、resize 或退出时，应按平台文档设计，避免把 Unix PTY 假设机械复制到 Windows。

内部设计：[LOCAL_SHELL.md](../modules/LOCAL_SHELL.md)。

## 串口标准

TauTerm 的 RS-232/RS-485 UI 和字节传输依赖平台串口 API；电气/物理标准本身由 TIA 等标准机构维护。

- ANSI/TIA-232 系列：TIA 官方 Standards Store 查询入口 https://tiaonline.org/what-we-do/standards/
- TIA-485 系列：同上。

这些标准可能需要付费授权。仓库只保存编号和适用范围，不复制正文。

TauTerm 的虚拟串口桥接只提供逻辑字节通道，不应宣称模拟物理电压、总线终端、电气时序或完整 RS-485 多点物理层。

内部设计：[SERIAL.md](../modules/SERIAL.md)、[TRANSFER.md](../modules/TRANSFER.md)。

## XMODEM / YMODEM / ZMODEM

这些协议来自早期串行通信生态，并不是 IETF RFC。实现时应参考原作者/历史协议文档，而不是根据现有某个终端软件的行为反推协议。

- XMODEM/YMODEM Protocol Reference — Chuck Forsberg 编辑的 1986/1988 协议参考（公开历史镜像可用于核对）：https://techheap.packetizer.com/communications/modems/xmodem-ymodem_reference.html
- The ZMODEM Inter Application File Transfer Protocol — Chuck Forsberg / Omen Technology：https://techheap.packetizer.com/communications/modems/zmodem8.html

TauTerm 实现 CRC、block size、batch metadata、retry/resume、ZMODEM header/subpacket 等行为时，应同时用互操作测试验证，因为历史实现存在扩展和兼容差异。

内部设计：[TRANSFER.md](../modules/TRANSFER.md)。

## AT 命令/响应

- ITU-T V.250 (07/2003), Serial asynchronous automatic dialling and control: https://www.itu.int/rec/T-REC-V.250/en

V.250 提供通用 ATtention 命令/响应语法基线，但实际蜂窝模组、调制解调器和设备会有大量厂商/3GPP 扩展。TauTerm 的 AT 工具只能声称解析自身明确支持的通用格式，不能把任意 `AT+...` 设备命令当成统一标准。

内部设计：[OBSERVABILITY_TOOLS.md](../modules/OBSERVABILITY_TOOLS.md)。

## Lua 自动化

### 权威来源

- Lua 5.4 Reference Manual: https://www.lua.org/manual/5.4/
- Lua license: https://www.lua.org/license.html
- mlua official crate docs: https://docs.rs/mlua/

TauTerm 使用 `mlua` 的 `lua54` + `vendored` 配置。脚本 API、sandbox 和线程/生命周期限制属于 TauTerm 自己的合同；Lua 语言语义以 Lua 5.4 官方手册为准。

内部设计：[AUTOMATION.md](../modules/AUTOMATION.md)。
