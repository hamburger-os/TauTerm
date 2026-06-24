# zmodem-file-transfer

## Purpose

Defines ZMODEM file transfer protocol implementation, including binary frame encoding (ZBIN/ZBIN32 with ZDLE escaping), sliding-window flow control, 32-bit CRC, adaptive block sizing up to 8KB, crash recovery with resume, and multi-file batch transfer.

## Requirements

### Requirement: ZMODEM 会话初始化与能力协商
系统 SHALL 支持 ZMODEM 发送和接收的 ZRQINIT/ZRINIT 初始化握手，遵循 lrzsz `getzrxinit`/`sendzsinit` 标准流程。

#### Scenario: 发送方发起 ZRQINIT
- **WHEN** 发送方启动 ZMODEM 发送
- **THEN** 发送方 SHALL 发送 ZRQINIT 帧（类型 0），包含能力标志（CANFDX、CANOVIO、CANFC32 等），等待接收方 ZRINIT 响应

#### Scenario: 接收方响应 ZRINIT
- **WHEN** 接收方收到发送方的 ZRQINIT 帧
- **THEN** 接收方 SHALL 发送 ZRINIT 帧（类型 1），声明自身能力（CANFDX、CANFC32、ESCCTL 等）

#### Scenario: 使用 32 位 CRC 协商
- **WHEN** 发送方和接收方均声明 CANFC32 能力
- **THEN** 双方 SHALL 使用 ZBIN32 帧格式（32 位 CRC）进行后续数据传输

#### Scenario: 超时退化为 ZRQINIT 重传
- **WHEN** 发送方发送 ZRQINIT 后未在超时内收到 ZRINIT
- **THEN** 发送方 SHALL 重传 ZRQINIT 最多 4 次，每次间隔约 10 秒

### Requirement: ZMODEM 帧编码与解码
系统 SHALL 实现 ZMODEM 二进制帧（ZBIN/ZBIN32）和十六进制帧（ZHEX）的编解码，支持 ZDLE 控制字符转义。

#### Scenario: 二进制帧编码
- **WHEN** 发送方需要发送 ZDATA 帧
- **THEN** 帧格式 SHALL 为 `ZPAD(2) ZDLE ZBIN type f3 f2 f1 f0 <data> crc1 crc2 ZDLE ZCRCE|ZCRCG|ZCRCQ|ZCRCW`

#### Scenario: 32 位 CRC 二进制帧编码
- **WHEN** 使用 ZBIN32 帧格式
- **THEN** 帧格式 SHALL 为 `ZPAD(2) ZDLE ZBIN32 type f3 f2 f1 f0 <data> crc1 crc2 crc3 crc4 ZDLE ZCRCE|ZCRCG|ZCRCQ|ZCRCW`

#### Scenario: ZDLE 控制字符转义
- **WHEN** 帧数据中包含 ZDLE（0x18）、XON（0x11）、XOFF（0x13）、CR（0x0d）等控制字符
- **THEN** 这些字符 SHALL 以 `ZDLE ^ 0x40` 转义序列编码

#### Scenario: 十六进制帧解码（回退模式）
- **WHEN** 接收方收到 ZPAD ZPAD ZHEX 前缀
- **THEN** 接收方 SHALL 以十六进制解码帧头和 CRC，格式为 `ZPAD ZPAD ZHEX <hex_encoded_hdr> <hex_encoded_crc> CR LF [XON]`

### Requirement: ZMODEM 数据流控与滑动窗口
系统 SHALL 实现 ZMODEM 滑动窗口流控机制，支持批量和流式数据传输。

#### Scenario: ZCRCG 连续帧
- **WHEN** 发送方连续发送多个 ZDATA 帧
- **THEN** 各帧 SHALL 以 ZCRCG（'i'）结束，表示"CRC 后帧继续，无需 ACK"，窗口大小由 `Zrwindow` 控制

#### Scenario: ZCRCW 窗口关闭帧
- **WHEN** 发送方需要接收方确认（窗口满或文件结束）
- **THEN** 帧 SHALL 以 ZCRCW（'k'）结束，表示"CRC 后期待 ZACK"

#### Scenario: ZCRCQ 查询帧
- **WHEN** 发送方定期请求位置确认（每 `Txwspac` 字节）
- **THEN** 帧 SHALL 以 ZCRCQ（'j'）结束，表示"CRC 后帧继续，期待 ZACK"

#### Scenario: 接收方 NAK 重传
- **WHEN** 接收方检测到帧 CRC 不匹配
- **THEN** 接收方 SHALL 发送 ZRPOS（类型 9）帧，携带最后正确接收的偏移量，触发发送方重传

#### Scenario: 自适应块大小
- **WHEN** ZMODEM 数据传输进行中
- **THEN** 块大小 SHALL 从 1024 字节开始，成功传输后逐步增大至最大 8192 字节；遇到错误时减小至 1024 字节

### Requirement: ZMODEM 文件元数据交换（ZFILE）
系统 SHALL 支持 ZFILE 帧传输文件名、大小、修改时间和传输选项。

#### Scenario: 发送文件元数据
- **WHEN** 发送方需要传输文件
- **THEN** 发送方 SHALL 发送 ZFILE 帧（类型 4），负载包含文件名（以 NULL 结尾）、文件大小（十进制字符串）、修改时间（八进制 Unix 时间戳）、模式（八进制）、已发送字节数（用于续传）和剩余字节数，各字段以空格分隔

#### Scenario: 接收方跳过文件
- **WHEN** 接收方收到不需要的文件（如文件已存在且更新）
- **THEN** 接收方 SHALL 以 ZSKIP（类型 5）响应

#### Scenario: 接收方接受文件
- **WHEN** 接收方接受文件传输
- **THEN** 接收方 SHALL 以 ZRPOS（类型 9）响应，携带偏移量 0（从头开始）或非零（断点续传）

### Requirement: ZMODEM 断点续传
系统 SHALL 支持 ZMODEM 断点续传（crash recovery），允许从中断处恢复传输。

#### Scenario: 断点续传发起
- **WHEN** 接收方检测到已存在同名文件且发送方请求续传（ZCRESUM 标志）
- **THEN** 接收方 SHALL 检查现有文件大小，在 ZRPOS 帧中指示从哪个偏移量恢复传输

#### Scenario: 无断点文件
- **WHEN** 接收方接收新文件或发送方未启用续传
- **THEN** 接收方 SHALL 在 ZRPOS 帧中指示偏移量 0（从头开始）

### Requirement: ZMODEM 批次结束握手
系统 SHALL 支持 ZEOF/ZFIN 结束握手正确结束批次传输。

#### Scenario: 单文件结束（ZEOF）
- **WHEN** 发送方完成一个文件的传输
- **THEN** 发送方 SHALL 发送 ZEOF 帧（类型 11）；接收方以 ZRINIT（类型 1）响应，请求下一个文件

#### Scenario: 批次结束（ZFIN）
- **WHEN** 发送方完成所有文件传输
- **THEN** 发送方 SHALL 发送 ZFIN 帧（类型 8）；接收方以 ZFIN 响应；双方以 `OO`（"Over and Out"）字符序列结束会话

#### Scenario: 发送方中止（ZABORT）
- **WHEN** 发送方需要中止传输
- **THEN** 发送方 SHALL 发送 ZABORT 帧（类型 7），随后发送 5 个连续 CAN 字节
