# file-transfer (Delta)

## Purpose

修改文件传输要求——从仅支持串口 YModem 端口移交，扩展为三策略传输架构（Inline / SideChannel / SeparateConnection），支持 SFTP、SCP 和 FTP 传输协议。

## MODIFIED Requirements

### Requirement: YModem 文件发送
系统必须支持通过活跃会话连接，使用 YModem 协议从主机向远程设备发送文件。对于串口会话，使用 Inline 策略（端口移交）。取消通道必须存储在 SessionHandle 中，在整个传输生命周期内保持有效。

#### Scenario: 通过串口发送单个文件
- **WHEN** 用户在串口会话中选择单个文件并启动 YModem 发送
- **THEN** Transfer Manager 检测到 channel 支持 handoff，使用 Inline 策略。I/O 循环暂停，端口移交给 YModem 发送器。文件必须以 1024 字节块传输，带 CRC-16 错误校验。

#### Scenario: 批量发送多个文件
- **WHEN** 用户选择多个文件并启动 YModem 批量发送
- **THEN** 每个文件必须依次发送块 0（文件元数据），随后发送文件数据块，批量传输必须以空块 0 结束

#### Scenario: 取消进行中的 Inline 传输
- **WHEN** 用户在活跃的 YModem 传输期间点击"取消"
- **THEN** 传输中止，端口立即归还给 I/O 循环，会话状态恢复为 "connected"。取消信号通道必须在传输开始前创建并存储。

#### Scenario: 传输错误恢复
- **WHEN** 从接收方收到 NAK（0x15）表示块损坏
- **THEN** 系统必须重新发送最后一个块，最多重试 10 次，之后以错误信息标记传输失败

### Requirement: SFTP 文件传输（SideChannel 策略）
系统必须支持通过 SSH 会话的 SFTP 子系统进行文件传输。SFTP 传输使用 SideChannel 策略——在 SSH 会话内打开独立 SFTP 子通道，不影响终端 I/O。

#### Scenario: 通过 SSH 会话上传文件
- **WHEN** 用户在 SSH 会话中启动 SFTP 文件上传
- **THEN** Transfer Manager 检测到 channel 不支持 handoff，使用 SideChannel 策略。SSH 插件打开 SFTP 子通道，传输文件。终端会话继续正常运行。

#### Scenario: SFTP 下载大文件并显示进度
- **WHEN** 用户通过 SFTP 下载一个 500MB 的文件
- **THEN** 系统必须实时显示传输进度（文件名、已传输字节/总字节、速度），与 YModem 使用完全相同的 `transfer-progress` 事件格式

### Requirement: FTP 文件传输（SeparateConnection 策略）
系统必须支持通过 FTP 插件进行文件传输。FTP 传输使用 SeparateConnection 策略——控制连接保持活跃，数据连接独立建立和关闭。

#### Scenario: FTP 被动模式下载
- **WHEN** 用户在 FTP 会话中启动文件下载
- **THEN** Transfer Manager 使用 SeparateConnection 策略，发送 PASV 命令，建立数据连接到返回的地址/端口，传输文件，关闭数据连接，控制连接保持活跃

### Requirement: 传输进度显示
所有三种传输策略必须使用统一的进度事件格式。进度条必须包含流光扫光动画效果。传输历史记录必须包含协议字段标识使用的传输协议。

#### Scenario: SFTP 传输进度动画
- **WHEN** SFTP 文件传输进行中
- **THEN** 进度条必须包含从左向右的流光扫光动画，配合百分比数字显示，事件格式与 YModem 完全一致

#### Scenario: 传输历史区分协议
- **WHEN** 一次 YModem 传输和一次 SFTP 传输都记录到历史
- **THEN** 历史记录条目 SHALL 显示对应的协议标签（📦 YModem、🔒 SFTP）

### Requirement: 文件传输界面
系统必须在界面中提供专用的文件传输面板，用于发起和监控所有策略的传输。面板必须支持拖拽上传（Dropzone）。面板必须根据活跃会话的协议能力显示可用的传输操作。

#### Scenario: 串口会话显示 YModem 传输选项
- **WHEN** 活跃会话是串口类型
- **THEN** 传输面板 SHALL 显示 YModem/XModem/ZModem 发送和接收按钮

#### Scenario: SSH 会话显示 SFTP 传输选项
- **WHEN** 活跃会话是 SSH 类型
- **THEN** 传输面板 SHALL 显示 SFTP 上传/下载按钮和文件浏览器入口

#### Scenario: 拖拽文件自动选择传输协议
- **WHEN** 用户在 SSH 会话中拖拽文件到窗口
- **THEN** 系统 SHALL 自动选择 SFTP 作为传输协议并启动上传

## ADDED Requirements

### Requirement: Transfer strategy auto-selection
The system SHALL automatically select the appropriate transfer strategy (Inline, SideChannel, or SeparateConnection) based on the session's channel capabilities and protocol.
The user SHALL NOT be required to manually choose a strategy.

#### Scenario: Auto-select Inline for serial
- **WHEN** any transfer is initiated on a serial session
- **THEN** Inline strategy SHALL be selected automatically

#### Scenario: Auto-select SideChannel for SSH
- **WHEN** any transfer is initiated on an SSH session
- **THEN** SideChannel strategy SHALL be selected automatically
