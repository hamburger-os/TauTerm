# Transport Runtime

## 目标

TauTerm 的传输层只负责建立、拥有和驱动字节/数据报资源。它不感知插件 UI、会话卡片、日志展示或具体协议语义。上层通过 DataPlane 使用传输，因而不需要知道底层驱动是阻塞串口、同步 PTY 还是异步网络库。

## 分层

```text
UI / Tauri IPC
    ↓
Session Runtime
    ↓
Protocol
    ↓
Transport Runtime
    ↓
Serial / TCP / UDP / PTY / protocol-native stream
```

硬性边界：

- Transport 不依赖 Tauri `AppHandle`，不 emit UI 事件。
- Protocol 不实现通用 session 生命周期、日志或 Tab 管理。
- Session Runtime 不解析 Modbus、Telnet 等协议字段。
- 插件不得自行复制串口打开、TCP connect/listen、UDP bind 等底层逻辑。

## I/O 语义

Transport 不使用一个万能接口硬塞所有能力。基础语义分为：

- byte stream
- datagram
- stream listener

`read` 必须显式区分 `Data / Idle / Eof`。不得再通过“不同 I/O loop 对 `Ok(0)` 的不同解释”表达 EOF。

## DataPlane

单个 transport runtime 永远拥有真实底层资源。外部只能通过 cloneable DataPlane handle：

- `write` / `flush`
- `subscribe` 接收数据事件
- `shutdown`
- 可选能力句柄，例如 TerminalControl
- `acquire_exclusive` 获取独占数据面

消费者（终端、脚本、日志、虚拟端口、协议解析器）通过订阅/运行时分发协作，禁止 `Mutex<Vec<callback>>` 回调树。

共享模式的 `write` 和终端 resize 是**有界入队操作**：调用成功表示 transport actor 已接管该命令，不表示物理驱动已经完成 I/O。这样同步 Tauri IPC 的高频终端输入/窗口调整不会等待串口/TCP/SSH 的 read slice 或远端 I/O，从而避免阻塞应用主线程。队列已满、独占租约占用或 runtime 已退出仍必须立即返回结构化错误；命令真正执行后的驱动错误由 actor 统一发布 `Closed` 事件并结束 DataPlane，Session Runtime 再把它转换为结构化断开事件。

需要逐次确认写入结果的所有权敏感路径（当前为 X/Y/ZModem 等 exclusive lease）不使用上述共享入队语义，而由 `ExclusiveIo` 保留写入 ACK。不要为了“统一接口”把这种确认式阻塞重新带回普通终端发送路径。

## Exclusive Lease

X/Y/ZModem 等必须独占串行数据面的功能使用 lease，而不是把真实 port 从 I/O loop 中 handoff：

```text
Shared -> Exclusive(owner) -> Shared
```

独占期间普通发送被拒绝，RX 只路由到 lease。资源所有权始终留在 transport runtime，因此不存在 `StubChannel`、端口 downcast 或归还失败导致端口丢失的问题。租约从申请开始即通过 DataPlane 的原子 admission 状态阻止新的共享写入，避免“Acquire 已入队但 ACK 尚未返回”窗口中混入普通终端数据。

## 阻塞驱动

“上层统一异步/事件化契约”不意味着强迫底层全部换成异步库。`serialport`、部分 PTY 等阻塞 API 可以由专用 worker 驱动；差异只存在于 transport 内部。

协议原生 async 驱动（当前 SSH）由 `AsyncBridgeDriver` 自有 Tokio runtime 驱动。任何依赖 Tokio reactor 的 future/timer 都必须在该 runtime 的上下文中创建并 poll，不能在普通 DataPlane OS 线程上先构造 `tokio::time` future 再交给 `block_on`。空闲读取使用短 read slice 让 actor 周期性处理共享写入、resize 和 shutdown；该 slice 属于 transport 内部调度参数，不得泄漏到 Session/UI。

## 能力

PTY resize、文件服务、多 peer 等不是 byte stream 的必备方法，必须通过独立 capability 暴露。调用者拿不到 capability 即表示不支持，不使用空实现伪装支持。

## 错误

Transport 错误是结构化语义：resolve、connect、bind、timeout、permission、device-not-found、device-busy、remote-closed、connection-reset、cancelled、io。协议层可以包装为协议错误；前端本地化不得依赖解析后端错误字符串。

共享命令的“已入队”和“已执行”必须区分：入队失败直接返回调用方；入队后的驱动失败通过 `Closed` 进入统一断开链路。不能一边向调用方返回失败、一边让 runtime 继续伪装为 connected。

## 资源关闭

关闭顺序：停止新命令 -> 取消协议任务 -> 释放 exclusive lease -> 请求 transport shutdown -> 等待 runtime 退出 -> 释放底层 handle。任何后台 worker 必须有可验证的退出路径，不能只依赖“线程之后大概会结束”。

DataPlane 的 `connected` / exclusive 运行态必须在正常退出和 panic unwind 两条路径都清零。Session receive subscription 在未请求 shutdown 的情况下突然关闭，应作为 transport actor 异常终止上报，而不是静默结束并让 UI 保持绿色连接态。

## Modbus

Modbus RTU/ASCII 复用 Serial transport；Modbus TCP 复用 TCP transport。Modbus codec、transaction、polling、server state machine 保持在 Modbus plugin 内，Transport 不包含 CRC、LRC、MBAP、Unit ID 或功能码知识。

## 代码锚点

- `src-tauri/src/transport/mod.rs`
- `src-tauri/src/transport/runtime.rs`
- `src-tauri/src/transport/stream.rs`
- `src-tauri/src/transport/serial.rs`
- `src-tauri/src/transport/tcp.rs`
- `src-tauri/src/transport/udp.rs`
