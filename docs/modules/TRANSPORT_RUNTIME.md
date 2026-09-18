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

单个 transport runtime 是真实底层资源的唯一生命周期 owner。共享模式下资源由 DataPlane actor 持有；进入 Exclusive lease 后，同一个通用 byte-stream driver 临时移动到 lease owner，释放 lease 后再归还 actor。外部不能直接取得具体 `serialport`、socket 或 PTY handle，只能通过 cloneable DataPlane handle：

- `write`
- `subscribe` 接收数据事件
- `shutdown`
- 可选能力句柄，例如 TerminalControl
- `acquire_exclusive` 获取独占数据面

消费者（终端、脚本、日志、虚拟端口、协议解析器）通过订阅/运行时分发协作，禁止 `Mutex<Vec<callback>>` 回调树。

每个 DataPlane subscription 都使用**有界 backlog**，并在创建时携带稳定的 consumer identity 供诊断日志定位具体消费者。通用消费者使用默认消息容量；对 burst 特征有明确需求的消费者可以显式指定自己的 message capacity，但容量必须有限且不得退化成无界队列。actor 发布数据时不能因为某个消费者变慢而阻塞底层 I/O，也不能为了保住消费者而让内存无界增长；订阅被摘除时会记录结构化终止原因，例如 backlog exceeded（含该订阅容量）或 runtime stopped，消费者不得再把所有 channel disconnect 模糊成同一种错误。

需要透明字节完整性的消费者必须让自己的下游阻塞与 DataPlane pump 隔离，并根据协议/线速选择有依据的有限 burst 容量；如果 subscription 本身仍被摘除，则把明确的终止原因作为自身失败向上层暴露，不能静默丢 chunk 后继续伪装正常。关闭广播同样使用非阻塞发送：队列已满的消费者直接摘除，不能让 transport actor 在 shutdown 路径上死锁。

普通共享写入和终端 resize 使用**确认式 ACK**：调用结果只在 transport actor 实际执行底层驱动操作后完成，因此 TX 日志、终端 TX 展示和错误返回不会把“已排队”误当成“已经写入设备”。Transport 同时提供同步与异步等待入口，但两者用途严格区分：同步入口只允许 worker / Rust 内部阻塞路径使用；WebView/Tauri 高频路径必须通过 `src-tauri/src/ipc_transport.rs` 的 async command 和 SessionIo async API 等待 ACK，绝不能让同步 Tauri command 在应用主线程上执行 `recv()`。

async 前端路径只在短临界区完成命令排队，然后释放异步顺序锁并等待 actor ACK。队列已满、独占租约占用或 runtime 已退出会立即返回结构化错误；共享模式底层 write/resize 失败则同时完成当前 ACK 并让 actor 进入统一 `Closed` 生命周期，Session Runtime 随后产生结构化断开事件。因此既不会冻结 UI，也不会出现“前端显示发送成功、后台稍后才发现写失败”的双重事实源。

## Exclusive Lease

X/Y/ZModem 等必须独占主字节流的功能使用通用 driver lease，而不是下转型、复制打开或把具体 `serialport::SerialPort` 暴露给协议：

```text
Shared:    DataPlane actor ──owns──> BlockingByteStream
                               │
                               │ acquire_exclusive
                               ▼
Exclusive: protocol worker ──owns──> BlockingByteStream + unread handoff bytes
                               │
                               │ release / Drop
                               ▼
Shared:    DataPlane actor ──owns──> BlockingByteStream
```

独占申请首先通过 DataPlane 的原子 admission 状态阻止新的共享 write/resize，并禁止 actor 再启动新的物理 read；actor 随后把完整 `Box<dyn BlockingByteStream>` 移交给 lease。若独占申请发生时已有一次 read 在进行，该 read 返回但尚未发布给共享订阅者的字节必须进入 handoff buffer，并与 driver 一起交给 lease。启动阶段尚未投递给任何订阅者的 startup bytes 同样属于未消费输入，也随 lease 一起移交。协议 worker 读取时先消费这些 prefetched bytes，再直接执行 driver 的 `read / write_all / flush`。

这一交接规则保证所有权切换是无损的：远端恰好在切换边界发送的 `C`、`NAK`、ZModem 初始化帧等不能被终端提前消费或被传输适配层清空。Inline 适配层因此禁止在取得 lease 后无条件 purge/flush RX；若协议本身需要丢弃噪声或跨文件残留，只能按该协议的同步规则在协议模块内部有限处理。

Exclusive 期间 actor 不持有 byte-stream driver，不进行后台 read-ahead，也不能执行普通写入或终端 resize；它只保留订阅与生命周期控制。lease 释放时必须把**同一个 driver 对象**归还 actor并等待接收确认。若 lease 尚有未消费的 prefetched bytes，这些字节必须与 driver 一并归还并重新进入共享接收流，之后才重新开放共享 admission。禁止重新引入 `StubChannel`、具体串口 downcast、重新打开端口、每次读写 IPC proxy 或平行的第二套 Session I/O owner。

传输期间的 driver I/O 错误属于当前协议任务：错误返回给 X/Y/ZModem，由任务结束路径释放 lease 并归还 driver；不能因为一次 exclusive 协议写失败就让 actor提前把整个 Session 判定为 `Closed`。归还后共享 I/O 是否仍可工作由后续真实 driver 操作决定。共享模式自身发生物理 I/O 错误时，仍进入统一 Session 断开生命周期。

关闭请求可能发生在 Exclusive 期间。actor 此时先记录 shutdown intent 并确认控制请求，等 lease 归还 driver 后执行实际 driver shutdown 并退出；不能等待自己当前并不持有的资源，也不能绕过 lease 并发关闭底层 handle。

## 阻塞驱动

“上层统一异步/事件化契约”不意味着强迫底层全部换成异步库。`serialport`、部分 PTY 等阻塞 API 可以由专用 worker 驱动；差异只存在于 transport 内部。

Raw Serial 的 read deadline 是 transport actor 的调度切片，不是协议数据包的 write deadline。Windows `COMMTIMEOUTS` 必须在端口打开时一次性配置为**读写非对称**：读取保持短总超时，使共享 actor 能及时处理 write、exclusive 和 shutdown；写入使用按 baud rate 与帧位数推导的每字节预算，再叠加固定调度裕量，使实际 deadline 随单次 `WriteFile` payload 长度增长。这样 128 B / 1 KiB X/YModem 块以及更大的串口写入不会继承短 read slice，同时也不需要在每个协议块前后反复调用 `SetCommTimeouts`。POSIX 串口继续使用平台原生阻塞语义。协议层不得为了绕过 transport deadline 自行修改底层串口 timeout。

这一超时模型只描述物理 I/O 上界，不替代协议自身的 ACK、NAK、CRC、重试或取消时限。Windows 写超时应覆盖至少一个保守倍数的理论线速时间，并保留有限固定裕量；发生超时时仍作为当前真实 driver I/O 失败返回，不能把部分写入伪装成成功。

协议原生 async 驱动（当前 SSH）由 `AsyncBridgeDriver` 自有 Tokio runtime 驱动。任何依赖 Tokio reactor 的 future/timer 都必须在该 runtime 的上下文中创建并 poll，不能在普通 DataPlane OS 线程上先构造 `tokio::time` future 再交给 `block_on`。空闲读取使用短 read slice 让 actor 周期性处理共享写入、resize 和 shutdown；该 slice 属于 transport 内部调度参数，不得泄漏到 Session/UI。

## 能力

PTY resize、文件服务、多 peer 等不是 byte stream 的必备方法，必须通过独立 capability 暴露。调用者拿不到 capability 即表示不支持，不使用空实现伪装支持。

## 错误

Transport 错误是结构化语义：resolve、connect、bind、timeout、permission、device-not-found、device-busy、remote-closed、connection-reset、cancelled、io。协议层可以包装为协议错误；前端本地化不得依赖解析后端错误字符串。

共享命令的“已排队”和“已执行”必须区分：排队失败直接返回调用方；成功排队后，async Tauri command 继续等待 actor 的确认结果。共享模式底层驱动失败既返回给当前调用方，也通过 `Closed` 进入统一断开链路；不能返回失败后仍让 runtime 伪装为 connected，也不能在尚未执行物理写入时提前记录 TX 成功。Exclusive lease 则直接返回 driver I/O 结果给协议 worker，其错误域由该协议任务收敛，不能自动越权升级为 Session 断开。

## 资源关闭

正常关闭顺序：停止新命令 -> 取消协议任务 -> 释放 exclusive lease -> 请求/完成 transport shutdown -> 等待 runtime 退出 -> 释放底层 handle。若 shutdown intent 在 lease 活动期间先到达，actor 记录该 intent，lease 归还 driver 后立即完成 driver shutdown，再退出 runtime。任何后台 worker 必须有可验证的退出路径，不能只依赖“线程之后大概会结束”。

DataPlane 的 `connected` / exclusive 运行态必须在正常退出和 panic unwind 两条路径都清零。Session receive subscription 在未请求 shutdown 的情况下突然关闭，应作为 transport actor 异常终止上报，而不是静默结束并让 UI 保持绿色连接态。

Tauri async command 需要等待 OS 线程退出时，必须先释放 SessionStore 等共享锁，再通过 `spawn_blocking` 执行 `join()`；不得直接占用 async runtime worker 等待线程退出。`close_channel` 的 WebView 边界遵循这一规则。

## Modbus

Modbus RTU/ASCII 复用 Serial transport；Modbus TCP 复用 TCP transport。Modbus codec、transaction、polling、server state machine 保持在 Modbus plugin 内，Transport 不包含 CRC、LRC、MBAP、Unit ID 或功能码知识。

## 代码锚点

- `src-tauri/src/ipc_transport.rs`
- `src-tauri/src/session/io.rs`
- `src-tauri/src/transport/mod.rs`
- `src-tauri/src/transport/runtime.rs`
- `src-tauri/src/transport/stream.rs`
- `src-tauri/src/transport/serial.rs`
- `src-tauri/src/transport/tcp.rs`
- `src-tauri/src/transport/udp.rs`
