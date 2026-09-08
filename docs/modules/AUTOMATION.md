# 发送与自动化设计

## 目标

TauTerm 的发送能力既要支持人工调试，也要支持命令面板、自动回复和脚本。不同入口必须共享同一个 Session 发送语义，避免“手动发送能工作、脚本却走另一条路径”。

## 当前方案

全局 SendBar 是插件能力，而不是所有 Session 的固定 UI。Serial、SSH、Telnet、Network Debug 等声明支持时可使用；Local Shell、TFTP、iperf、TRDP 由各自工作流完成操作，不强制显示发送栏。

对支持 SendBar 的 Session，基础发送、命令面板、自动回复和脚本共享 `CommHandle`/Session 发送边界。字符集转换只作用于文本路径；HEX/raw byte 路径保持原始字节。

Command Set、Auto Reply Config 与 Lua Script 是可复用工程资产，不再把浏览器本地存储作为权威来源。当前统一保存到 Rust ConfigStore 的 `assets.*` 命名空间；内置示例在首次装载时与用户资产按稳定 name/id 合并，后续修改仍只写同一个持久层。WebView 内的 `assetStore` 是这组 global asset 的单一内存协调层：同一 key 的写入串行执行，持久化成功后广播给所有已挂载 Session 的 SendBar，失败时回滚到最近一次已确认快照并发出全局错误提示，避免隐藏 Session 用旧副本覆盖新资产。这个 global asset ownership 是完整 `TauWorkspace.assets` 之前的当前实现，未来 Named Workspace 可以在同一资产模型上增加 workspace override，而不是再引入第二套格式。

Network Debug 还会把当前发送目标同步到公共发送上下文，使人工发送和脚本默认指向同一目标，同时允许脚本使用显式目标 API。

## 数据流

```mermaid
flowchart LR
  Manual["手动发送"] --> Send["Session 发送边界"]
  Command["命令面板"] --> Send
  Reply["自动回复"] --> Send
  Script["Lua 脚本"] --> Send
  Target["当前目标 / 编码"] --> Send
  Send --> Protocol["协议 I/O"]
```

## 设计边界

- SendBar 显示能力由插件 manifest/注册信息定义，Session 配置只能在支持范围内关闭，不能给不支持的插件强行开启。
- 自动回复与脚本必须受 Session 生命周期约束，断开后不能继续使用失效的运行时 handle。
- 文本编码和 raw bytes 明确分流，不能对 HEX/raw 数据做字符集二次转换。
- 自动化不能绕过协议模块的目标选择、安全确认或连接状态。
- 脚本 VM/状态按 Session 隔离，避免不同连接之间共享不可控状态。
- 资产定义（Command/Rule/Script）与运行时执行状态分离；`isRunning`、临时日志、当前 VM/handle 不进入持久化资产。
- 多个 Session 可以各自拥有 SendBar 运行态，但 global asset definition 不能各自维护不可见的陈旧副本；资产保存失败必须显式可见，不能吞掉 IPC/磁盘错误。

## 代码锚点

- `src/components/SendBar/`
- `src/components/SendBar/assetStore.ts`
- `src-tauri/src/kernel/config_store.rs`
- `src-tauri/src/kernel/comm_handle.rs`
- `src-tauri/src/kernel/script_engine/`
- `src/context/SessionContext.tsx`

## 何时更新本文

修改 SendBar 能力模型、发送目标、编码路径、自动回复、Lua API、脚本隔离或自动化与 Session 生命周期的关系时，必须同步更新本文。

Lua 5.4 与终端控制序列的权威入口见 [TERMINAL_SERIAL_AUTOMATION.md](../knowledge/TERMINAL_SERIAL_AUTOMATION.md)。
