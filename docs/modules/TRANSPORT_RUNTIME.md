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

## Exclusive Lease

X/Y/ZModem 等必须独占串行数据面的功能使用 lease，而不是把真实 port 从 I/O loop 中 handoff：

```text
Shared -> Exclusive(owner) -> Shared
```

独占期间普通发送被拒绝，RX 只路由到 lease。资源所有权始终留在 transport runtime，因此不存在 `StubChannel`、端口 downcast 或归还失败导致端口丢失的问题。

## 阻塞驱动

“上层统一异步/事件化契约”不意味着强迫底层全部换成异步库。`serialport`、部分 PTY 等阻塞 API 可以由专用 worker 驱动；差异只存在于 transport 内部。

## 能力

PTY resize、文件服务、多 peer 等不是 byte stream 的必备方法，必须通过独立 capability 暴露。调用者拿不到 capability 即表示不支持，不使用空实现伪装支持。

## 错误

Transport 错误是结构化语义：resolve、connect、bind、timeout、permission、device-not-found、device-busy、remote-closed、connection-reset、cancelled、io。协议层可以包装为协议错误；前端本地化不得依赖解析后端错误字符串。

## 资源关闭

关闭顺序：停止新命令 -> 取消协议任务 -> 释放 exclusive lease -> 请求 transport shutdown -> 等待 runtime 退出 -> 释放底层 handle。任何后台 worker 必须有可验证的退出路径，不能只依赖“线程之后大概会结束”。

## Modbus

Modbus RTU/ASCII 复用 Serial transport；Modbus TCP 复用 TCP transport。Modbus codec、transaction、polling、server state machine 保持在 Modbus plugin 内，Transport 不包含 CRC、LRC、MBAP、Unit ID 或功能码知识。