# xmodem-file-transfer

## Purpose

Defines XMODEM file transfer protocol implementation, including standard XMODEM (128B blocks + checksum), XMODEM-1k (1024B blocks + CRC-16), and XMODEM-CRC (128B blocks + CRC-16) variants, with single-file send and receive support.

## ADDED Requirements

### Requirement: XMODEM 文件发送
系统 SHALL 支持通过串口使用 XMODEM 协议向远程设备发送单个文件，遵循 lrzsz `wcs`/`wctx`/`wcputsec` 标准发送流程。

#### Scenario: 标准 XMODEM 发送（128B + 校验和）
- **WHEN** 用户选择单个文件并以 XMODEM 协议启动发送，接收方以 NAK（0x15）发起传输
- **THEN** 发送方 SHALL 以 128 字节块传输文件数据，每块附带 1 字节校验和；块 1 为首个数据块（无块 0 元数据）；收到 ACK 后发送下一块；收到 NAK 后重传当前块最多 10 次

#### Scenario: XMODEM-CRC 发送（128B + CRC-16）
- **WHEN** 用户选择单个文件并以 XMODEM 协议启动发送，接收方以 'C'（0x43）发起 CRC 模式传输
- **THEN** 发送方 SHALL 以 128 字节块传输文件数据，每块附带 2 字节 CRC-16/CCITT；其余行为与标准 XMODEM 一致

#### Scenario: XMODEM-1k 发送（1024B + CRC-16）
- **WHEN** 用户选择单个文件并以 XMODEM 协议启动发送，接收方以 'G'（0x47）发起 1k 模式传输
- **THEN** 发送方 SHALL 以 1024 字节块传输文件数据，每块附带 2 字节 CRC-16/CCITT；STX 作为块头（替代 SOH）

#### Scenario: 不足块填充
- **WHEN** 最后一个数据块的字节数不足块大小（128 或 1024）
- **THEN** 发送方 SHALL 以 0x1A（Ctrl-Z / CPMEOF）填充至满块大小

#### Scenario: EOT 握手
- **WHEN** 所有文件数据块已发送并确认
- **THEN** 发送方 SHALL 发送 EOT（0x04），等待接收方以 ACK 响应；若超时或收到 NAK，重传 EOT 最多 10 次

#### Scenario: 发送方 NAK 探测
- **WHEN** 发送方启动 XMODEM 发送，等待接收方初始信号
- **THEN** 发送方 SHALL 等待接收方发送 NAK/C/G 作为就绪信号，超时 30 秒后报错

### Requirement: XMODEM 文件接收
系统 SHALL 支持通过串口使用 XMODEM 协议从远程设备接收单个文件，遵循 lrzsz `wcrx`/`wcgetsec` 标准接收流程。

#### Scenario: 标准 XMODEM 接收（校验和模式）
- **WHEN** 用户启动 XMODEM 接收，系统以 NAK 发起传输
- **THEN** 接收方 SHALL 读取 128 字节块，验证 1 字节校验和；校验通过则 ACK 并写入磁盘；校验失败则 NAK 请求重传

#### Scenario: XMODEM-CRC 接收
- **WHEN** 用户启动 XMODEM 接收，系统以 'C' 发起 CRC 模式传输
- **THEN** 接收方 SHALL 读取 128 字节块，验证 2 字节 CRC-16；校验通过则 ACK 并写入磁盘

#### Scenario: XMODEM-1k 接收
- **WHEN** 用户启动 XMODEM 接收，系统以 'G' 发起 1k 模式传输
- **THEN** 接收方 SHALL 解析 STX 头为 1024 字节块，验证 CRC-16；校验通过则 ACK 并写入磁盘

#### Scenario: EOT 处理
- **WHEN** 接收方读取到 EOT
- **THEN** 接收方 SHALL 关闭输出文件，发送 ACK，完成接收

#### Scenario: CAN 取消处理
- **WHEN** 接收方连续收到两个 CAN（0x18）字节
- **THEN** 接收方 SHALL 中止传输，删除不完整的文件

#### Scenario: 重复包检测
- **WHEN** 接收方收到与上一块相同块号的块（表示 ACK 丢失导致发送方重传）
- **THEN** 接收方 SHALL 发送 ACK 但跳过数据写入

### Requirement: XMODEM 变体协商
发送方 SHALL 在启动阶段（`getnak` 阶段）自适应接收方请求的 XMODEM 变体。

#### Scenario: 自适应 CRC 模式
- **WHEN** 发送方在探测阶段收到 'C' 字节
- **THEN** 发送方 SHALL 切换为 CRC-16 校验模式，块大小保持 128 字节

#### Scenario: 自适应 1k 模式
- **WHEN** 发送方在探测阶段收到 'G' 字节
- **THEN** 发送方 SHALL 切换为 CRC-16 校验模式，块大小设为 1024 字节

#### Scenario: 退化为标准模式
- **WHEN** 发送方在探测阶段收到 NAK
- **THEN** 发送方 SHALL 使用 128 字节块 + 1 字节校验和
