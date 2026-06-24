# file-transfer

## Purpose

Delta spec: 扩展文件传输功能以支持 XModem、YModem 和 ZModem 三种协议选择，同时按 lrzsz 标准重构 YMODEM 收发逻辑。

## MODIFIED Requirements

### Requirement: YModem 文件发送
系统必须支持通过活跃串口连接，使用 YModem 协议从主机向远程设备发送文件。取消通道必须存储在 SessionHandle 中，在整个传输生命周期内保持有效。发送逻辑 SHALL 遵循 lrzsz `wcs`/`wctx`/`wcputsec` 标准流程，包括块 0 元数据、数据块 1k/128B 自适应、EOT 握手。

#### Scenario: 发送单个文件
- **WHEN** 用户选择单个文件并启动 YModem 发送
- **THEN** 文件必须以 1024 字节块传输，带 CRC-16 错误校验；当剩余字节数 ≤ 896 时块大小 SHALL 切换为 128 字节（对齐 lrzsz `wctx` 行为）；传输必须完成并收到接收方的成功确认

#### Scenario: 批量发送多个文件
- **WHEN** 用户选择多个文件并启动 YModem 批量发送
- **THEN** 每个文件必须依次发送块 0（文件元数据：名称\0大小\0mtime\0mode\0\0），随后发送文件数据块，批量传输必须以空块 0 结束以表示批次结束

#### Scenario: 传输进度显示
- **WHEN** YModem 文件发送进行中
- **THEN** 界面必须显示当前文件名、已传输字节/总字节、传输速度以及进度条

#### Scenario: 取消进行中的传输
- **WHEN** 用户在活跃的 YModem 传输期间点击"取消"
- **THEN** `cancel_transfer` 命令通过 `SessionHandle.cancel_transfer_tx` 发送信号，传输通过发送 CAN 序列（两个连续的 0x18 字节）中止，串口保持打开以供正常终端使用。取消信号通道必须在传输开始前创建并存储，在传输完成或取消后清除。

#### Scenario: 取消通道不在传输前被释放
- **WHEN** `send_files_ymodem` 命令被调用
- **THEN** 取消通道的发送端存储在 SessionHandle 中而非立即丢弃，确保取消信号仅在用户主动取消或传输完成后触发

#### Scenario: 传输错误恢复
- **WHEN** 从接收方收到 NAK（0x15）表示块损坏
- **THEN** 系统必须重新发送最后一个块，最多重试 10 次，之后以错误信息标记传输失败

#### Scenario: 协议类型选择发送
- **WHEN** 用户选择 XModem/YModem/ZModem 协议之一并启动文件发送
- **THEN** 系统 SHALL 根据 `TransferProtocolType` 枚举值创建对应的协议处理器（`XModem`/`YModem`/`ZModem` 结构体），通过 `TransferProtocol` trait 调用 `send_files` 方法

### Requirement: YModem 文件接收
系统必须支持通过活跃串口连接，使用 YModem 协议从远程设备接收文件。接收到的文件数据必须实际写入磁盘。接收逻辑 SHALL 遵循 lrzsz `wcrx`/`wcgetsec` 标准流程，包括重复包检测和 CRC 模式协商（'C' 探测）。

#### Scenario: 接收单个文件
- **WHEN** 远程设备启动 YModem 发送且用户接受传入传输
- **THEN** 文件必须被接收，数据块解码后写入用户指定的下载目录，块以 ACK（0x06）确认，CRC 验证通过，文件保存到磁盘

#### Scenario: 批量接收多个文件
- **WHEN** 远程设备以 YModem 批量模式发送多个文件
- **THEN** 每个文件必须按块 0 元数据中指定的原始文件名接收并保存到磁盘，接收到空块 0 时批量传输完成

#### Scenario: 接收的数据写入磁盘
- **WHEN** 接收端成功 CRC 校验一个数据块
- **THEN** 该块的数据负载必须通过 `std::fs::File::write_all()` 写入下载目录中的文件

#### Scenario: 接收进度显示
- **WHEN** YModem 文件接收进行中
- **THEN** 界面必须显示当前文件名、已接收字节/总字节、传输速度以及进度条

#### Scenario: 拒绝传入传输
- **WHEN** 远程设备启动 YModem 发送且用户拒绝
- **THEN** 传输必须被拒绝，串口保持打开以供正常终端使用

#### Scenario: 协议类型选择接收
- **WHEN** 用户选择 XModem/YModem/ZModem 协议之一并启动文件接收
- **THEN** 系统 SHALL 根据 `TransferProtocolType` 枚举值创建对应的协议处理器，通过 `TransferProtocol` trait 调用 `receive_files` 方法

### Requirement: 传输进度显示
进度条必须包含流光扫光动画效果。

#### Scenario: 传输进度动画
- **WHEN** 文件传输进行中（不限于 YModem）
- **THEN** 进度条必须包含从左向右的流光扫光动画，配合百分比数字显示

### Requirement: 文件传输界面
系统必须在界面中提供专用的文件传输面板，用于发起和监控传输。面板必须支持拖拽上传（Dropzone）和协议选择。

#### Scenario: 打开文件传输面板
- **WHEN** 用户点击状态栏中的文件传输按钮或按下 Ctrl+Shift+F
- **THEN** 必须从底部滑出一个玻璃面板，包含"发送文件"、"接收文件"按钮、协议选择器（XModem/YModem/ZModem）和传输历史列表

#### Scenario: 拖拽文件上传
- **WHEN** 用户从桌面拖拽一个文件进入 TauTerm 窗口
- **THEN** 整个窗口变暗（rgba(0,0,0,0.4) 遮罩），文件传输面板自动滑出并产生呼吸闪烁的青色边框，面板中央显示 "⚡ Drop to Transfer" 提示文字

#### Scenario: 放置文件开始传输
- **WHEN** 用户在 Dropzone 中松开放置的文件
- **THEN** 面板播放一道扫光动画（Scan-line Sweep），随后自动启动所选协议的发送

#### Scenario: 拖拽离开取消
- **WHEN** 用户将文件拖出 TauTerm 窗口
- **THEN** 遮罩消失，面板恢复正常状态，不启动传输

#### Scenario: 选择要发送的文件
- **WHEN** 用户点击"发送文件"
- **THEN** 必须打开原生文件选择对话框，允许选择一个或多个文件

#### Scenario: 配置下载目录
- **WHEN** 用户首次点击"接收文件"
- **THEN** 系统必须提示用户选择下载目录，该目录必须被记住以备后续传输使用

#### Scenario: 传输历史
- **WHEN** 文件传输完成（成功或失败）
- **THEN** 必须向传输历史中添加一条记录，显示文件名、协议类型、方向（发送/接收）、大小、状态和时间戳

### Requirement: Dropzone 视觉反馈
文件传输面板必须作为 Dropzone 工作，提供完整的拖拽视觉反馈。

#### Scenario: 拖拽进入窗口
- **WHEN** 文件被拖入 TauTerm 窗口
- **THEN** 系统检测到拖拽事件，整个窗口覆盖半透明暗色遮罩

#### Scenario: 拖拽悬停在传输面板上
- **WHEN** 拖拽的文件悬停在文件传输面板上方
- **THEN** 面板边框以青色高频呼吸闪烁（1s 周期），背景略微变亮

## ADDED Requirements

### Requirement: 协议选择器
文件传输面板 SHALL 提供协议选择器，允许用户在 XModem、YModem 和 ZModem 之间切换。

#### Scenario: 显示可用协议
- **WHEN** 用户打开文件传输面板
- **THEN** 协议选择器 SHALL 列出 XMODEM、YMODEM、ZMODEM 三个选项，默认选中 YMODEM

#### Scenario: 切换协议
- **WHEN** 用户点击协议选择器选择不同协议
- **THEN** 后续发送/接收操作 SHALL 使用所选协议；协议名称 SHALL 出现在传输历史中
