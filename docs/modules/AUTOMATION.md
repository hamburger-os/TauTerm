# 发送与自动化设计

## 目标

TauTerm 的发送能力既要支持人工调试，也要支持命令面板、自动回复和脚本。不同入口必须共享同一个 Session 发送语义，避免“手动发送能工作、脚本却走另一条路径”。

## 当前方案

全局 SendBar 是插件能力，而不是所有 Session 的固定 UI。Serial、SSH、Telnet、Network Debug 等声明支持时可使用；Local Shell、TFTP、iperf、TRDP 由各自工作流完成操作，不强制显示发送栏。

对支持 SendBar 的 Session，基础发送、命令面板、自动回复和脚本共享 `CommHandle`/Session 发送边界。基础发送的手动发送与重复发送共用同一 payload 编码入口：文本模式只在这里追加 CRLF/LF/CR/None，HEX 模式直接生成 raw bytes；重复发送采用串行背压，只有上一次底层写入完成后才安排下一次发送，避免定时器堆叠并发写入。

SendBar 的四个模式不再全部常驻 DOM，只挂载当前模式。真正需要跨模式保留的输入、选择和执行所有权由每个 Session 自己的 `SendBarContext` 保存；Command Set、Auto Reply Config 与 Lua Script 的定义则是跨 Session 复用的工程资产。这样“会话 UI/运行态”和“工程资产定义”不会因为懒挂载或多分屏而混在一起。

Command Set、Auto Reply Config 与 Lua Script 不再把浏览器本地存储作为权威来源，统一保存到 Rust ConfigStore 的 `assets.*` 命名空间；WebView 内的 `assetStore` 是这些 global asset 的单一内存协调层，同一 key 的写入串行执行，持久化成功后才广播给已挂载 Session，失败时回滚到最近一次已确认快照并显式报告错误。内置示例只在对应资产存储从未初始化时播种一次；显式空数组是合法用户状态，用户删除全部示例后不会因为重启或新建 Session 自动复活，只能通过“加载内置示例”再次加入。

工程资产的“当前选择”仍是会话态：每个 SendBar 可以选不同的命令集、自动回复配置或脚本。持久化的 active key 只作为新挂载 SendBar 的默认值，已挂载 Session 不会跟随其他 Session 的选择跳转。Lua 编辑器代码是本会话草稿：共享脚本更新时，干净编辑器可以跟随已保存资产刷新；存在本地未保存修改时，不会被另一个 Session 的保存隐式覆盖。

自动回复与 Lua Script 启动时使用不可变运行快照。SendBar 从“启动请求发出”开始占有执行权，直到启动失败、停止成功或会话断开后才释放；这段时间禁止切换模式，也冻结会改变当前运行视图的共享资产回落。这样即使另一个 Session 同时删除、重命名或修改同一资产，当前执行中的规则/脚本与界面仍保持一致，停止后再按最新资产目录回落。命令面板执行同样基于启动时选中命令的串行快照，底层发送尚未返回时不会因为“停止”立即允许第二条执行链重启。

Network Debug 的目标选择由 SessionContext 拥有；`TargetBar` 只负责展示和选择，独立的目标同步桥接负责把 TCP/UDP server 当前目标传给后端脚本引擎并显式报告同步失败。目标选择可以在已保存或断开的会话中保留，但只有 `network` 插件会话进入 `connected` 后才允许跨 IPC 同步到运行时 `NetworkSideChannel`；断开、连接中或已被新目标取代的异步同步不会产生陈旧错误提示。因此人工发送、命令面板、自动回复和脚本仍使用同一个目标语义，同时不会让纯前端会话配置越过后端运行时生命周期。

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
- 自动回复与脚本必须受 Session 生命周期约束，断开后不能继续使用失效的运行时 handle；启动中的异步请求也必须失效，不能在断线后重新把界面置为运行中。
- Network Debug 的目标选择属于会话状态，但后端目标同步属于运行时副作用：仅 `connected` 的 Network Debug server 会话可调用 `set_network_send_target`；后端继续严格校验 side channel，不以静默成功掩盖真实运行时异常。
- 文本编码和 raw bytes 明确分流，不能对 HEX/raw 数据做字符集二次转换；所有基础发送路径共用同一 payload 构造函数。
- 重复发送和命令序列必须尊重底层写入背压，不能用不等待 Promise 的固定间隔制造重叠发送。
- 自动化不能绕过协议模块的目标选择、安全确认或连接状态。
- 脚本 VM/状态按 Session 隔离，避免不同连接之间共享不可控状态。
- 资产定义（Command/Rule/Script）与会话选择、编辑草稿、执行所有权分离；`isRunning`、临时日志、当前 VM/handle 不进入持久化资产。
- 多个 Session 可以共享 global asset definition，但已挂载 Session 的 active selection 不互相跟随；存在本地草稿或运行快照时，其他 Session 的资产写入不能隐式改写当前执行语义。
- 破坏性二元确认统一使用公共 `ConfirmDialog`，取消/确认动作与主题样式保持一致，不在 SendBar 内复制临时确认控件。

## 代码锚点

- `src/components/SendBar/`
- `src/components/SendBar/SendBarContext.tsx`
- `src/components/SendBar/sendPayload.ts`
- `src/components/SendBar/assetStore.ts`
- `src/components/SendBar/assetValidation.ts`
- `src/components/SendBar/useNetworkSendTargetSync.ts`
- `src-tauri/src/kernel/config_store.rs`
- `src-tauri/src/kernel/comm_handle.rs`
- `src-tauri/src/kernel/script_engine/`
- `src/context/SessionContext.tsx`

## 回归检查

`npm run check:sendbar` 验证基础发送 payload、重复发送背压、导入 schema、会话态/工程资产边界、Network Debug 目标同步生命周期、执行锁和公共确认弹窗等关键合同，并作为 CI 的前端 hygiene 检查之一。

## 何时更新本文

修改 SendBar 能力模型、发送目标、编码路径、工程资产所有权、自动回复、Lua API、脚本隔离或自动化与 Session 生命周期的关系时，必须同步更新本文。

Lua 5.4 与终端控制序列的权威入口见 [TERMINAL_SERIAL_AUTOMATION.md](../knowledge/TERMINAL_SERIAL_AUTOMATION.md)。