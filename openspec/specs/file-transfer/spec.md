# file-transfer

## Purpose

定义文件传输功能要求，包括多协议传输架构（YModem/XModem/ZModem/SFTP/SCP/FTP）、三策略自动选择（Inline / SideChannel / SeparateConnection）、传输进度显示、传输界面和 Dropzone 拖拽上传。

## Requirements

### Requirement: YModem 文件发送
系统必须支持通过活跃会话连接，使用 YModem 协议从主机向远程设备发送文件。对于串口会话，使用 Inline 策略（端口移交）。对于其他传输类型，由 Transfer Manager 自动选择策略。取消通道必须存储在 SessionHandle 中，在整个传输生命周期内保持有效。

#### Scenario: 通过串口发送单个文件
- **WHEN** 用户在串口会话中选择单个文件并启动 YModem 发送
- **THEN** Transfer Manager 检测到 channel 支持 handoff，使用 Inline 策略。I/O 循环暂停，端口移交给 YModem 发送器。文件必须以 1024 字节块传输，带 CRC-16 错误校验。

#### Scenario: 批量发送多个文件
- **WHEN** 用户选择多个文件并启动 YModem 批量发送
- **THEN** 每个文件必须依次发送块 0（文件元数据：名称和大小），随后发送文件数据块，批量传输必须以空块 0 结束以表示批次结束

#### Scenario: 传输进度显示
- **WHEN** YModem 文件发送进行中
- **THEN** 界面必须显示当前文件名、已传输字节/总字节、传输速度以及进度条

#### Scenario: 取消进行中的 Inline 传输
- **WHEN** 用户在活跃的 YModem 传输期间点击"取消"
- **THEN** 传输中止，端口立即归还给 I/O 循环，会话状态恢复为 "connected"。取消信号通道必须在传输开始前创建并存储。

#### Scenario: 取消通道不在传输前被释放
- **WHEN** `send_files` 命令被调用
- **THEN** 取消通道的发送端存储在 SessionHandle 中而非立即丢弃，确保取消信号仅在用户主动取消或传输完成后触发

#### Scenario: 传输错误恢复
- **WHEN** 从接收方收到 NAK（0x15）表示块损坏
- **THEN** 系统必须重新发送最后一个块，最多重试 10 次，之后以错误信息标记传输失败

### Requirement: 传输进度显示
所有三种传输策略必须使用统一的进度事件格式。进度条必须包含流光扫光动画效果。传输历史记录必须包含协议字段标识使用的传输协议。

#### Scenario: 传输进度动画
- **WHEN** 文件传输进行中（任意协议）
- **THEN** 进度条必须包含从左向右的流光扫光动画，配合百分比数字显示，事件格式统一

#### Scenario: 传输历史区分协议
- **WHEN** 一次 YModem 传输和一次 SFTP 传输都记录到历史
- **THEN** 历史记录条目 SHALL 显示对应的协议标签（📦 YModem、🔒 SFTP）

### Requirement: YModem 文件接收
系统必须支持通过活跃串口连接，使用 YModem 协议从远程设备接收文件。接收到的文件数据必须实际写入磁盘。

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

### Requirement: 文件传输界面
系统必须在界面中提供专用的文件传输面板，用于发起和监控所有策略的传输。面板必须支持拖拽上传（Dropzone）。面板必须根据活跃会话的协议能力显示可用的传输操作。

#### Scenario: 打开文件传输面板
- **WHEN** 用户点击状态栏中的文件传输按钮或按下 Ctrl+Shift+F
- **THEN** 必须从底部滑出一个玻璃面板，包含传输操作按钮和传输历史列表

#### Scenario: 串口会话显示 YModem 传输选项
- **WHEN** 活跃会话是串口类型
- **THEN** 传输面板 SHALL 显示 YModem/XModem/ZModem 发送和接收按钮

#### Scenario: SSH 会话显示 SFTP 传输选项
- **WHEN** 活跃会话是 SSH 类型
- **THEN** 传输面板 SHALL 显示 SFTP 上传/下载按钮和文件浏览器入口

#### Scenario: 拖拽文件自动选择传输协议
- **WHEN** 用户在 SSH 会话中拖拽文件到窗口
- **THEN** 系统 SHALL 自动选择 SFTP 作为传输协议并启动上传

#### Scenario: 拖拽文件上传
- **WHEN** 用户从桌面拖拽一个文件进入 TauTerm 窗口
- **THEN** 整个窗口变暗（rgba(0,0,0,0.4) 遮罩），文件传输面板自动滑出并产生呼吸闪烁的青色边框，面板中央显示 "⚡ Drop to Transfer" 提示文字

#### Scenario: 放置文件开始传输
- **WHEN** 用户在 Dropzone 中松开放置的文件
- **THEN** 面板播放一道扫光动画（Scan-line Sweep），随后自动启动 YModem 发送

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
- **THEN** 必须向传输历史中添加一条记录，显示文件名、方向（发送/接收）、大小、状态和时间戳

### Requirement: Dropzone 视觉反馈
文件传输面板必须作为 Dropzone 工作，提供完整的拖拽视觉反馈。

#### Scenario: 拖拽进入窗口
- **WHEN** 文件被拖入 TauTerm 窗口
- **THEN** 系统检测到拖拽事件，整个窗口覆盖半透明暗色遮罩

#### Scenario: 拖拽悬停在传输面板上
- **WHEN** 拖拽的文件悬停在文件传输面板上方
- **THEN** 面板边框以青色高频呼吸闪烁（1s 周期），背景略微变亮

### Requirement: SFTP 文件传输（SideChannel 策略）
系统必须支持通过 SSH 会话的 SFTP 子系统进行文件传输。SFTP 传输使用 SideChannel 策略——在 SSH 会话内打开独立 SFTP 子通道，不影响终端 I/O。

#### Scenario: 通过 SSH 会话上传文件
- **WHEN** 用户在 SSH 会话中启动 SFTP 文件上传
- **THEN** Transfer Manager 检测到 channel 不支持 handoff，使用 SideChannel 策略。SSH 插件打开 SFTP 子通道，传输文件。终端会话继续正常运行。

#### Scenario: SFTP 下载大文件并显示进度
- **WHEN** 用户通过 SFTP 下载大文件
- **THEN** 系统必须实时显示传输进度（文件名、已传输字节/总字节、速度），使用相同的 `transfer-progress` 事件格式

### Requirement: FTP 文件传输（SeparateConnection 策略）
系统必须支持通过 FTP 插件进行文件传输。FTP 传输使用 SeparateConnection 策略——控制连接保持活跃，数据连接独立建立和关闭。

#### Scenario: FTP 被动模式下载
- **WHEN** 用户在 FTP 会话中启动文件下载
- **THEN** Transfer Manager 使用 SeparateConnection 策略，发送 PASV 命令，建立数据连接到返回的地址/端口，传输文件，关闭数据连接，控制连接保持活跃

### Requirement: Transfer strategy auto-selection
The system SHALL automatically select the appropriate transfer strategy (Inline, SideChannel, or SeparateConnection) based on the session's channel capabilities and protocol.
The user SHALL NOT be required to manually choose a strategy.

#### Scenario: Auto-select Inline for serial
- **WHEN** any transfer is initiated on a serial session
- **THEN** Inline strategy SHALL be selected automatically

#### Scenario: Auto-select SideChannel for SSH
- **WHEN** any transfer is initiated on an SSH session
- **THEN** SideChannel strategy SHALL be selected automatically
