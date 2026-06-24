## MODIFIED Requirements

### Requirement: YModem 文件发送
系统必须支持通过活跃串口连接，使用 YModem 协议从主机向远程设备发送文件。取消通道必须存储在 SessionHandle 中，在整个传输生命周期内保持有效。块 0 元数据必须使用 lrzsz 标准格式；CRC-16 必须使用零填充传输；EOT 握手必须使用 lrzsz 标准单路径。

#### Scenario: 发送单个文件
- **WHEN** 用户选择单个文件并启动 YModem 发送
- **THEN** 文件必须以 1024 字节块传输，使用 lrzsz 标准 CRC-16 零填充方法（`updcrc(0, updcrc(0, crc))`）进行错误校验，块 0 以 `filename\0size mtime mode 0 1 size` 格式编码，传输必须完成并收到接收方的 ACK 确认

#### Scenario: 批量发送多个文件
- **WHEN** 用户选择多个文件并启动 YModem 批量发送
- **THEN** 每个文件必须依次发送块 0（lrzsz 格式文件元数据），随后发送文件数据块，批量传输必须以空块 0 结束以表示批次结束，发送方不得等待空块 0 的 ACK

#### Scenario: 传输进度显示
- **WHEN** YModem 文件发送进行中
- **THEN** 界面必须显示当前文件名、已传输字节/总字节、传输速度以及进度条

#### Scenario: 取消进行中的传输
- **WHEN** 用户在活跃的 YModem 传输期间点击"取消"
- **THEN** `cancel_transfer` 命令通过 `SessionHandle.cancel_transfer_tx` 发送信号，传输通过发送 lrzsz 标准 CAN 序列（10 个 0x18 字节后跟 8 个 0x08 退格字符）中止，串口保持打开以供正常终端使用。取消信号通道必须在传输开始前创建并存储，在传输完成或取消后清除。

#### Scenario: 取消通道不在传输前被释放
- **WHEN** `send_files_ymodem` 命令被调用
- **THEN** 取消通道的发送端存储在 SessionHandle 中而非立即丢弃，确保取消信号仅在用户主动取消或传输完成后触发

#### Scenario: 传输错误恢复
- **WHEN** 从接收方收到 NAK（0x15）表示块损坏
- **THEN** 系统必须重新发送最后一个块，最多重试 10 次，之后以错误信息标记传输失败

### Requirement: YModem 文件接收
系统必须支持通过活跃串口连接，使用 YModem 协议从远程设备接收文件。接收到的文件数据必须实际写入磁盘。CRC 验证必须使用 lrzsz 标准的前馈验证方法。

#### Scenario: 接收单个文件
- **WHEN** 远程设备启动 YModem 发送且用户接受传入传输
- **THEN** 文件必须被接收，数据块解码后写入用户指定的下载目录，块以 ACK（0x06）确认，CRC 通过 lrzsz 标准前馈验证（将接收到的两个 CRC 字节输入 CRC 引擎，验证结果为零），文件保存到磁盘

#### Scenario: 批量接收多个文件
- **WHEN** 远程设备以 YModem 批量模式发送多个文件
- **THEN** 每个文件必须按块 0 元数据中指定的原始文件名接收并保存到磁盘，接收到空块 0 时批量传输完成。接收方必须在文件之间发送 'C' 请求下一个块 0（对齐 lrzsz `wcrxpn()` 行为）

#### Scenario: 接收的数据写入磁盘
- **WHEN** 接收端成功通过 lrzsz 标准前馈 CRC 验证一个数据块
- **THEN** 该块的数据负载必须通过 `std::fs::File::write_all()` 写入下载目录中的文件

#### Scenario: 接收进度显示
- **WHEN** YModem 文件接收进行中
- **THEN** 界面必须显示当前文件名、已接收字节/总字节、传输速度以及进度条

#### Scenario: 拒绝传入传输
- **WHEN** 远程设备启动 YModem 发送且用户拒绝
- **THEN** 传输必须被拒绝，串口保持打开以供正常终端使用
