# Modbus 标准知识基线

本文是 TauTerm Modbus 实现的协议语义权威来源。实现、测试与 UI 术语必须以本页列出的公开规范为准；应用层便利功能不得改变线上的 Modbus 语义。

## 权威资料

- Modbus Organization, **MODBUS Application Protocol Specification V1.1b3**.
- Modbus Organization, **MODBUS over Serial Line Specification and Implementation Guide V1.02**.
- Modbus Organization, **MODBUS Messaging on TCP/IP Implementation Guide V1.0b**.
- 官方规范入口：<https://www.modbus.org/modbus-specifications>

若规范之间存在范围差异，Application Protocol 定义 PDU 与功能码；Serial Line Guide 定义 RTU/ASCII 串行封装、地址与时序；TCP/IP Guide 定义 MBAP 与 TCP 映射。

## TauTerm 支持边界

一级标准模式：

- Modbus RTU over serial
- Modbus ASCII over serial
- Modbus TCP

`RTU over TCP` 属工程兼容模式，只能放在“高级”区域，不得与 Modbus TCP 并列标注为标准模式。Modbus Security/TLS 是独立规范族，不隐式等同普通 Modbus TCP。

## 地址规则

TauTerm 内部唯一事实源是 PDU 中的 **0-based protocol address（0..65535）**。传统的 00001 / 10001 / 30001 / 40001 引用只属于 UI 显示法：

- 00001 -> Coil address 0
- 10001 -> Discrete Input address 0
- 30001 -> Input Register address 0
- 40001 -> Holding Register address 0

不得把 40001 等显示值存进协议模型，也不得在多个层级隐式 `-1`。

## 串行地址与广播

- Unit/Server address `0`：广播，仅串行链路使用；只允许写操作；服务器不回复，客户端不得等待响应。
- `1..247`：普通服务器地址。
- `248..255`：保留。

## 标准公开功能码

TauTerm 的标准功能覆盖至少包括：

- `0x01` Read Coils
- `0x02` Read Discrete Inputs
- `0x03` Read Holding Registers
- `0x04` Read Input Registers
- `0x05` Write Single Coil
- `0x06` Write Single Register
- `0x07` Read Exception Status（Serial）
- `0x08` Diagnostics（Serial）
- `0x0B` Get Comm Event Counter（Serial）
- `0x0C` Get Comm Event Log（Serial）
- `0x0F` Write Multiple Coils
- `0x10` Write Multiple Registers
- `0x11` Report Server ID（Serial）
- `0x14` Read File Record
- `0x15` Write File Record
- `0x16` Mask Write Register
- `0x17` Read/Write Multiple Registers
- `0x18` Read FIFO Queue
- `0x2B / MEI 0x0D` CANopen General Reference
- `0x2B / MEI 0x0E` Read Device Identification

常用读写功能放在主操作页；诊断、设备识别、文件记录、FIFO 与通用 MEI 放入次级/高级工作流，避免把功能码堆成按钮墙。Raw PDU 用于自定义或厂商功能码。

## 标准数量上限

核心校验必须在编码前完成：

- FC01 / FC02：1..2000 bits
- FC03 / FC04：1..125 registers
- FC0F：1..1968 coils
- FC10：1..123 registers

其它功能码按 V1.1b3 对各自 PDU 的字段范围与最大 PDU 长度校验。

## RTU

RTU ADU = address + PDU + CRC-16。CRC 在线上传输低字节在前。

帧边界属于串行链路语义，不能依赖 WebView 定时器或固定 UI 批处理周期。实现必须把字符时间/静默间隔、响应超时与帧收集放在后端协议运行时处理。一个串行主站事务同一时刻最多只有一个未完成请求。

## ASCII

ASCII ADU 使用 `:` 起始、十六进制字符编码、LRC、CRLF 结束。解析器应拒绝奇数十六进制字符、非法字符、错误 LRC、缺失终止符和超长/不完整帧。

## TCP

Modbus TCP ADU = MBAP header + PDU。MBAP 字段：

- Transaction Identifier：用于请求/响应匹配。
- Protocol Identifier：Modbus 必须为 0。
- Length：后续 Unit Identifier + PDU 的字节数。
- Unit Identifier：桥接场景中的单元地址。

TCP 是字节流：一次读取可能只有半帧，也可能含多帧。实现必须拥有增量缓冲器，根据 MBAP Length 拆分完整 ADU，不能假设一次 `read` 等于一帧。

## 响应验证

客户端至少验证：

- TCP Transaction Identifier / Protocol Identifier / Length
- Unit Identifier
- function code 或 exception function code
- byte count 与实际长度
- 写操作回显的地址、数量或值
- RTU CRC / ASCII LRC
- 针对请求类型的最小/精确 PDU 长度

结果语义必须区分：成功、Modbus Exception、协议错误、畸形响应、超时、传输错误、取消。

## 重试

读请求允许配置自动重试。写请求默认不得自动重试：响应超时并不证明设备没有执行写入，因此结果必须标记为“响应超时 · 写入结果未知”。若高级选项允许重试写入，必须明确提示重复写风险。

## 数据解释

协议层只处理 bit/register 与字节。应用层数据解释是独立能力，可支持 Bool、UInt/Int16/32/64、Float32/64、Hex/Binary、ASCII/UTF-8 byte view、bit field、scale/offset/unit，以及工程实践中的 ABCD/BADC/CDAB/DCBA 字节/字序组合。这些是设备数据布局约定，不应统称为“Modbus 大小端”。

## Client / Server

“全面 Modbus 调试”同时包含 Client/Master 与 Server/Slave Simulator。

服务器数据模型包含 Coils、Discrete Inputs、Holding Registers、Input Registers；写功能应更新可写模型。模拟器应支持标准异常、延时、无响应等故障注入，并保证串行广播不返回响应。

## Raw 模式

- **Raw PDU**：用户只提供 PDU；TauTerm 根据选择的传输自动生成 Unit/MBAP/CRC/LRC。
- **Raw ADU / Malformed Frame**：完全按用户字节发送，可故意构造错误 CRC/LRC、Length、Transaction ID、byte count 等。

两种模式必须分开，避免“自动修复”改变用户想发送的畸形帧。

## 轮询

Watch Table 调度器不得产生积压：一个条目的上一轮仍未完成时，下一 tick 只跳过或合并，不排队堆积。断连时暂停，重连后按持久化配置恢复调度。

## 测试最低门槛

实现变更至少覆盖：RTU/ASCII/TCP golden vectors、CRC/LRC、标准功能码、标准异常、地址转换、数量边界、TCP 粘包/拆包、多帧、非法 MBAP、RTU/ASCII 畸形帧、广播、读重试、写不自动重试、写超时未知结果、取消/断连、Watch 无积压、Server 数据模型与故障注入、数据解释/字序、会话恢复和资源释放。