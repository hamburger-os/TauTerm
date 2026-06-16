# architecture-polish

## Purpose

精简后端架构中过度抽象或未完成重构的设计，提升代码可读性和维护性。

## Requirements

### Requirement: TermSession Enum Replacement
`TermSession` trait 及其仅有的错误返回 stub 实现 SHALL 被替换为具体枚举：

```rust
enum SessionImpl {
    Serial(SerialSession),
    // Ssh(SshSession),  // 未来扩展
    // Telnet(TelnetSession),
}
```

`SessionManager::create_session()` SHALL 直接构造 `SessionImpl::Serial(SerialSession)` 而非通过 trait 方法。

#### Scenario: Session creation without trait
- **WHEN** 用户创建串口会话
- **THEN** `create_session` 直接实例化 `SerialSession`，不再经过 trait 的错误返回方法

#### Scenario: SessionImpl enum is extensible
- **WHEN** 未来添加 SSH 支持
- **THEN** 向 `SessionImpl` 枚举添加 `Ssh` 变体即可，无需修改 trait 定义

### Requirement: Cancel Channel Stored in SessionHandle
YModem 传输的取消 oneshot 通道 SHALL 从命令函数局部变量提升为 `SessionHandle` 的字段 `cancel_transfer_tx: Option<Sender<()>>`，由 `cancel_transfer` 命令触发。

#### Scenario: Cancel transfer via command
- **WHEN** 用户在前端点击"取消传输"
- **THEN** `cancel_transfer` 命令获取对应 session 的 `cancel_transfer_tx` 并发送取消信号，传输线程收到信号并中止

#### Scenario: Cancel channel lifecycle
- **WHEN** 传输正常完成或失败
- **THEN** `cancel_transfer_tx` 字段被置为 `None`，避免重用已关闭的通道

### Requirement: Named Constants for Magic Numbers
以下硬编码数值 SHALL 提取为命名常量：
- `open_port` 重试次数和延迟：`PORT_OPEN_RETRIES = 3`、`PORT_OPEN_RETRY_DELAY_MS = 100`、`PORT_STABILIZE_DELAY_MS = 30`
- `flush_port_buffer` 参数：`FLUSH_EMPTY_THRESHOLD = 3`、`FLUSH_MAX_ITERATIONS = 20`、`FLUSH_TIMEOUT_MS = 50`
- I/O 线程轮询间隔：`IO_THREAD_TICK_MS = 1`
- 会话关闭延迟：`SESSION_CLOSE_DELAY_MS = 100`（Windows 特定）
- YModem 最大重试：`YMODEM_MAX_RETRIES = 10`

#### Scenario: Constants are documentable
- **WHEN** 开发者需要调整串口重连参数
- **THEN** 在模块顶部找到命名常量并修改一处即可，无需搜索整个代码库

### Requirement: Shared read_byte_with_timeout for YModem
`YModemSender` 和 `YModemReceiver` 中重复的 `read_byte_with_timeout` 函数 SHALL 合并为共享的模块级自由函数。

#### Scenario: Single implementation
- **WHEN** 需要修改超时读取逻辑
- **THEN** 仅修改一处函数定义，发送和接收两端行为保持一致
