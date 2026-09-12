# 串口与设备调试设计

## 目标

串口模块把物理串口连接、文本/HEX 观察、自动化和文件传输整合成一个设备调试 Session，并在需要时把同一物理数据流桥接给外部工具。

## 当前方案

Serial 作为标准终端型协议接入公共 Session/DataPlane 核心。真正的串口 handle 由 `transport::serial` 打开并由 `DataPlaneRuntime` 独占持有；终端显示、SendBar、字符集、脚本、统计和断开都通过 `SessionDataPlane + SessionIo` 复用公共能力。

X/Y/ZModem 不再把物理串口从运行时取出再归还。传输启动时通过 `SessionIo::acquire_exclusive` 获得独占 lease，任务结束后 RAII 释放；底层串口所有权始终留在 DataPlane Runtime，因此普通发送、取消、断开和传输之间不存在第二套端口 ownership。

物理串口列表只在进入 Serial 配置页时按需刷新，并保留最近一次发现结果供表单立即显示。Windows 端口枚举可能受 SetupAPI、蓝牙设备或第三方驱动影响而变慢，因此枚举必须在后台 blocking worker 中运行，不能阻塞配置 UI。

### 串口端点数据契约

`EndpointInfo.name` 是真正用于连接的系统端点名，例如 `COM5`；`description` 只提供补充说明，不重复 `name`。USB 串口至少携带 VID/PID、serial number、manufacturer、product，并在 serial number 可用时生成稳定 device identity。驱动友好名若仅在末尾重复当前端口号，展示层会规范化掉重复后缀，但结构化 identity 中仍保留驱动原始字段。

COM/tty 名称是瞬时属性，不能把它当成未来 same-device reconnect 的唯一身份。蓝牙/PCI/未知串口也保留类型与 system port 元数据，但不伪造不存在的硬件唯一 ID。

## 虚拟串口模型

虚拟串口是平台能力而不是协议替代品：

- Windows 由受控的 com0com 后端创建端口对，生产安装场景优先通过特权服务执行；
- Linux/macOS 使用进程内 POSIX PTY 桥接，不依赖外部 helper。

上层统一使用“内部 bridge + 对外 external endpoint”的能力模型。Windows 的 com0com 一对端口中：

- `bridge_path` 只由 TauTerm 桥接线程打开，是内部资源，不进入普通串口选择列表，也不进入前端展示契约；
- `external_path` 是用户和其它软件应该打开的端点，例如 `COM21`；
- 前端不感知 `CNCA/CNCB`、bus 编号或 Windows 端口对实现细节。

典型数据流：

```text
物理 COM6 ⇄ TauTerm Serial DataPlane ⇄ 内部 COM20 ⇄ com0com ⇄ 对外 COM21
```

桥接只保证字节流转发，不模拟真实 UART 电气特性、调制解调器控制线或所有波特率行为。

## 虚拟端口所有权与清理

残留资源判断必须建立在明确所有权上，不能通过“驱动里存在一个 com0com bus”推断它属于 TauTerm。

直连 Windows 后端维护两层状态：

- `active_endpoints`：当前进程仍被活动 Session 持有的端点；
- 持久化 `owned_endpoints`：TauTerm 已创建且仍负责回收的端点，包括 active 和异常退出/权限不足后留下的资源。

严格定义：

```text
orphan = owned_endpoints - active_endpoints
```

由此得到以下生命周期规则：

1. 普通创建成功后立即登记 ownership；提权批量创建会在启动特权子进程前预登记目标 ownership，并在明确失败时回滚已创建端口；
2. 创建成功后把 bridge path 注册为内部不可见端点，并将 endpoint 转为 active；
3. 正常活动期间端点同时属于 owned 和 active，因此不是 orphan；
4. 外部程序断开 external endpoint 不改变父 Serial Session ownership；
5. 父 Serial Session 结束时尝试销毁端口对；成功后同时移除 active/owned 和内部隐藏注册；
6. 销毁暂时失败时只移除 active、保留 owned，此时才成为可恢复 orphan；
7. 进程异常退出后新进程没有 active owner，而持久化 owned 仍存在，因此可恢复清理；
8. 手动“清理残留端口”只能处理已证明属于 TauTerm 且当前非 active 的资源，禁止删除第三方/用户自行创建的 com0com bus；
9. 特权服务模式同样使用 ownership 模型，服务重启只恢复/清理有 ownership 证据的 TauTerm orphan；
10. com0com 驱动本身是系统级共享资源，与 TauTerm endpoint ownership 分开处理；无法确认系统级 driver ownership 时必须保留共享驱动。

持久化状态采用当前唯一 schema，不保留旧版 bus-only 兼容逻辑；预稳定阶段发现旧/损坏 schema 时只备份用于诊断，并重新建立当前模型。

## 关键数据流

```mermaid
flowchart LR
  Device["物理串口"] <--> DP["Serial DataPlane"]
  DP <--> Session["SessionIo / SessionDataPlane"]
  Session <--> UI["终端 / SendBar / 脚本"]
  Session <--> Transfer["ExclusiveIo → X/Y/ZModem"]
  Session <--> Bridge["内部虚拟端点"]
  Bridge <--> External["对外虚拟端点"]
```

## 设计边界

- 一个物理串口只有一个 Transport/DataPlane owner；Inline 文件传输通过 exclusive lease 临时获得访问权，不转移底层 handle。
- 虚拟串口创建失败不能让主串口连接的状态变成错误真相。
- 平台提权逻辑不得进入普通 Serial UI/协议语义。
- 自动化发送、编码与日志复用公共 Session 能力，不建立串口专属第二套实现。
- 当前采集设备 identity，但自动按 stable identity 重连仍是后续能力，不能提前宣传。
- orphan/cleanup 必须基于 TauTerm 所有权证据，不能以驱动全局枚举结果作为删除授权。
- 系统级共享驱动的卸载权限独立于 endpoint ownership。

## 代码锚点

- `src/plugins/serial/`
- `src-tauri/src/plugins/serial/`
- `src-tauri/src/transport/serial.rs`
- `src-tauri/src/transport/runtime.rs`
- `src-tauri/src/session/io.rs`
- `src-tauri/src/virtual_port/`
- `src/components/Layout/StatusBar.tsx`
- `src/hooks/useCom0comStatus.ts`

## 何时更新本文

修改串口连接模型、虚拟串口后端、端点可见性、资源所有权、DataPlane 所有权、传输集成方式或设备数据流时，必须同步更新本文。

共享 X/Y/ZModem 传输生命周期见 [TRANSFER.md](TRANSFER.md)；Transport 语义见 [TRANSPORT_RUNTIME.md](TRANSPORT_RUNTIME.md)；串口/PTY/电气标准依据见 [终端、串口与自动化知识索引](../knowledge/TERMINAL_SERIAL_AUTOMATION.md)。