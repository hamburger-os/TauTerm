# transfer-protocol-abstraction

## Purpose

Defines the protocol-agnostic transfer abstraction layer, including the `TransferProtocol` trait for unified X/Y/ZModem interface, shared CRC/checksum computation, I/O utilities, and common transfer event types.

## ADDED Requirements

### Requirement: TransferProtocol trait 定义
系统 SHALL 定义 `TransferProtocol` trait 统一 XMODEM、YMODEM、ZMODEM 三种协议的收发接口。

#### Scenario: 发送接口统一
- **WHEN** 调用方通过 `Box<dyn TransferProtocol>` 发送文件
- **THEN** 调用方 SHALL 调用 `send_files(port, files, on_progress, on_file_event, cancel) -> Result<Vec<BatchFileResult>>`，无需感知具体协议

#### Scenario: 接收接口统一
- **WHEN** 调用方通过 `Box<dyn TransferProtocol>` 接收文件
- **THEN** 调用方 SHALL 调用 `receive_files(port, download_dir, on_progress, on_file_event, cancel) -> Result<Vec<BatchFileResult>>`，无需感知具体协议

#### Scenario: XModem/YModem/ZModem 实现 trait
- **WHEN** 定义了 `XModem`、`YModem`、`ZModem` 结构体
- **THEN** 每个结构体 SHALL 实现 `TransferProtocol` trait 的 `send_files` 和 `receive_files` 方法

### Requirement: 共享 CRC 和校验和计算
系统 SHALL 在独立模块 `crc.rs` 中提供 CRC-16/CCITT、CRC-32 和 XMODEM 校验和计算函数，供所有协议模块复用。

#### Scenario: CRC-16/CCITT 计算
- **WHEN** 调用 `crc16_ccitt(data: &[u8]) -> u16`
- **THEN** 系统 SHALL 使用 CCITT 多项式 0x1021，初始值 0x0000，返回 16 位 CRC

#### Scenario: CRC-32 计算（ZMODEM 标准）
- **WHEN** 调用 `crc32_zmodem(data: &[u8]) -> u32`
- **THEN** 系统 SHALL 使用 ZMODEM 标准多项式 0xEDB88320，初始值 0xFFFFFFFF，返回 32 位 CRC

#### Scenario: XMODEM 校验和计算
- **WHEN** 调用 `checksum(data: &[u8]) -> u8`
- **THEN** 系统 SHALL 返回数据字节的算术和 mod 256

#### Scenario: 块 CRC 验证
- **WHEN** 调用 `crc16_ccitt_verify(data: &[u8], expected_crc: u16) -> bool`
- **THEN** 系统 SHALL 对比计算值与预期值，返回验证结果

### Requirement: 共享 I/O 工具函数
系统 SHALL 在独立模块 `io.rs` 中提供串口 I/O 工具函数，供所有协议模块复用。

#### Scenario: 超时读取单字节
- **WHEN** 调用 `read_byte_with_timeout(port, timeout_ms) -> Result<Option<u8>>`
- **THEN** 系统 SHALL 以 10ms 轮询间隔读取串口，总等待不超过 `timeout_ms`；收到字节返回 `Some(u8)`，超时返回 `None`

#### Scenario: 端口缓冲区刷新
- **WHEN** 调用 `flush_port_buffer(port)`
- **THEN** 系统 SHALL 读取并丢弃所有可用数据，直到连续 3 次读取为空（每次 50ms），确保缓冲区清空

#### Scenario: CAN 取消序列发送
- **WHEN** 调用 `send_cancel(port)`
- **THEN** 系统 SHALL 写入两个连续 CAN 字节（0x18 0x18），刷新输出缓冲区，等待 100ms

### Requirement: 公共传输事件类型
系统 SHALL 定义协议无关的公共事件类型，替代原有的 YModem 专用类型。

#### Scenario: FileTransferEvent 替代 YModemFileEvent
- **WHEN** 传输过程中发出文件事件
- **THEN** `FileTransferEvent` 枚举 SHALL 包含 `FileStart { file_name, file_index, total_files, file_size }` 和 `FileComplete { file_name, file_index, total_files, bytes_transferred, success, error }` 变体，适用于所有三种协议

#### Scenario: YModemFileEvent 向后兼容
- **WHEN** 前端代码引用 `YModemFileEvent`
- **THEN** `YModemFileEvent` SHALL 作为 `FileTransferEvent` 的 type alias 存在，标记 `#[deprecated]` 并在 1-2 版本后移除

#### Scenario: 协议类型枚举
- **WHEN** 调用方选择传输协议
- **THEN** `TransferProtocolType` 枚举 SHALL 包含 `XModem`、`YModem`、`ZModem` 变体，实现 `FromStr` 和 `Display`

#### Scenario: FileInfo 文件信息结构
- **WHEN** 调用方准备发送文件
- **THEN** `FileInfo` 结构体 SHALL 包含 `path: String`、`name: String`（文件名）、`size: u64`、`mtime: u64`（Unix 时间戳），从文件系统元数据填充
