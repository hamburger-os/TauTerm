# Serial Terminal (Delta)

## MODIFIED Requirements

### Requirement: 串口数据发送
系统必须允许用户在终端中输入并将按键发送到串口。I/O 通道必须使用带缓冲的 `sync_channel(32)` 替代无缓冲 rendezvous 通道。I/O 循环必须采用公平读写调度，读操作不再跳过写检查。

#### Scenario: 发送键入字符
- **WHEN** 用户在连接串口时输入字符
- **THEN** 每个字符必须实时通过串口发送，即使串口正在持续接收数据

#### Scenario: 发送特殊按键
- **WHEN** 用户按下 Enter、Tab、Escape 或方向键
- **THEN** 对应的控制序列必须通过串口发送，Enter 键必须在 100ms 内被写入设备

#### Scenario: 快速连续输入
- **WHEN** 用户在连接状态下快速连续输入 20 个字符
- **THEN** 所有字符必须被缓冲并通过串口发送，不阻塞 UI 线程

#### Scenario: 粘贴文本
- **WHEN** 用户在连接状态下从剪贴板粘贴多行文本
- **THEN** 粘贴的文本必须通过串口发送

### Requirement: 串口数据接收与显示
系统必须从打开的串口读取数据，并实时在终端仿真器中显示。接收循环必须与写入操作公平交替。

#### Scenario: 同时收发数据
- **WHEN** 串口同时接收数据和用户输入命令
- **THEN** 接收和发送必须交替进行，任何一方不被另一方阻塞超过 100ms

## ADDED Requirements

### Requirement: 多会话串口支持
系统必须支持同时打开多个串口会话，每个会话拥有独立的 I/O 线程和通道。

#### Scenario: 同时连接两个串口
- **WHEN** 用户在标签页 A 连接 COM3，在标签页 B 连接 COM5
- **THEN** 两个串口同时独立工作，互不干扰

#### Scenario: 一个会话断开不影响其他
- **WHEN** COM3 设备被物理拔出
- **THEN** COM3 的标签页显示断开状态，COM5 的标签页继续正常工作
