# Modbus 模块设计

## 目标

Modbus 是独立 custom Session，提供标准 Modbus RTU、ASCII、TCP 的 Client 调试、轮询监视、事务审计、Raw 调试和 Server Simulator。协议语义全部留在 Modbus 插件；串口/TCP 资源继续复用公共 Transport Runtime。

权威协议语义与实现依据见 [MODBUS 知识层](../knowledge/MODBUS.md)。本文件只描述 TauTerm 当前实现边界。

## 分层

```text
Modbus Session View
  ↓ Tauri commands
Modbus runtime
  ├─ Client transaction engine
  ├─ Watch scheduler
  └─ Server simulator
  ↓
Codec / validation
  ├─ PDU
  ├─ RTU (CRC)
  ├─ ASCII (LRC)
  └─ TCP (MBAP)
  ↓
Transport Runtime
  ├─ Serial
  └─ TCP
```

Transport 不知道 Unit ID、功能码、CRC/LRC、MBAP、异常码或寄存器模型；这些全部属于 Modbus 模块。

## Client

Client 会话在连接时建立一个 `DataPlaneRuntime`，事务引擎对它保持唯一 request/response 所有权。当前同一 Client 默认一次只允许一个 outstanding transaction：RTU/ASCII 由协议顺序性要求如此，TCP 调试器也保持默认 1 outstanding 以避免调试上下文被并发响应打散。

标准执行流程：

1. `ModbusRequest` 编码为 PDU；
2. 根据 RTU/ASCII/TCP 添加 ADU envelope；
3. 写入公共 DataPlane；
4. 按 transport/framing 规则组帧；
5. 严格校验 Unit/TID/PID/MBAP length/function/byte-count/echo/CRC/LRC；
6. 输出结构化 `TransactionResult` 并加入有界历史。

事务状态明确区分 `success / broadcast / modbus_exception / protocol_error / malformed_response / timeout / transport_error / cancelled`。写请求超时或 transport failure 时标记 `write_outcome_unknown`，避免把“未收到响应”错误解释为“设备一定没有执行写入”。

读取请求可以按配置重试；写请求默认不重试，只有显式启用时才允许复用 retry 次数。串口 Unit 0 只允许 write broadcast，发送后不等待响应。

## 功能码

常用读写界面覆盖：

- 01 Read Coils
- 02 Read Discrete Inputs
- 03 Read Holding Registers
- 04 Read Input Registers
- 05 Write Single Coil
- 06 Write Single Register
- 0F Write Multiple Coils
- 10 Write Multiple Registers
- 16 Mask Write Register
- 17 Read/Write Multiple Registers

Advanced 入口覆盖串口诊断和高级功能：07、08、0B、0C、11、14、15、18、2B/0D、2B/0E，以及 Raw PDU / exact Raw ADU。

Raw PDU 仍由 TauTerm 添加所选模式的 envelope；exact Raw ADU 则按用户提供字节原样发送，不做协议修正，并明确标记响应为未验证 raw 数据。

## 地址与值解释

内部 canonical 地址始终为 0..65535 的 PDU address。传统 `00001 / 10001 / 30001 / 40001` 只属于显示/输入辅助，不能进入底层协议状态。

Monitor 的值解释支持 Bool、UInt/Int16/32/64、Float32/64、ASCII/UTF-8、bit、scale/offset/unit 和 ABCD/BADC/CDAB/DCBA 等字节/字序组合。值解释发生在寄存器原始字节之上，不改变协议请求本身。

## Watch / Monitor

Watch row 保存稳定定义：请求、周期、值格式和显示名称。Scheduler 对每行按完成时刻重新安排下一次执行，不补偿错过的 tick，因此不会因慢设备产生 backlog/reentry。停止会话或显式 Stop 会终止轮询；运行态 value、latency、timestamp 不作为持久化配置。

## Transaction History

Client 历史是有界内存记录，保存时间戳、Unit、FC、TID、attempt、latency、TX/RX/PDU、status、exception 和 outcome-unknown 标记。它是调试审计运行态，不写入 Saved Session。

Server 请求也应以同一“有界运行态历史”原则记录；若 UI 展示 server request log，必须保持它与持久化配置分离。

## Server Simulator

Server 维护四个标准数据区：Coils、Discrete Inputs、Holding Registers、Input Registers。读请求从模型返回数据，写请求直接更新模型；TCP Server 支持多 client，串口 Server 处理 RTU/ASCII 顺序请求。

Server fault injection 是运行态能力，可动态设置：

- no response
- response delay
- forced Modbus exception

这些故障只改变响应行为，不改变 transport 所有权。

## 持久化边界

Saved Session 只保存可重建的稳定配置，例如：

- mode / role
- serial 或 TCP endpoint 配置
- unit id / timeout / retry 策略
- server max clients / 默认 fault 配置
- Monitor watch row 定义和值格式
- Server model 的用户配置初值（若启用持久化）

不能持久化 DataPlane、TID 计数器、running flag、poll timer、transaction history、当前请求、socket/serial handle 等运行态。

## UI

Modbus 使用独立 `customView`，不显示全局 SendBar。主要页面：

- Read / Write
- Monitor
- Transactions
- Advanced
- Server

Client/Server 角色只显示适用功能；exact Raw ADU 属于 Advanced 的显式高风险调试入口，不与普通请求混在一起。

连接配置页按“会话模式 / 连接参数 / 协议参数 / 高级”分区。TCP Client 使用远端主机语义，TCP Server 使用监听地址语义；RTU/ASCII 只显示串口相关字段。Client 专属的响应超时、重试和 TCP 连接超时不会出现在 Server 的主配置路径中。串口列表显示遵循 `端口 — 描述`，当描述与端口名相同则只显示一次，避免 `COM5 — COM5` 之类重复文本。

工作区采用“会话概览 / 功能页签 / 操作与结果内容”的层级。读写页将请求编辑器与执行结果分开，未执行事务时显示明确空状态；Server 以数据模型为主入口，而不是复用 Client 的请求表单。

### 会话身份

Modbus 的模式与角色必须在任何主要视图中可直接辨认：

- TCP Client：`Modbus TCP Client`
- TCP Server：`Modbus TCP Server`
- RTU Master：`Modbus RTU Master`
- RTU Slave Simulator：`Modbus RTU Slave`
- ASCII Master：`Modbus ASCII Master`
- ASCII Slave Simulator：`Modbus ASCII Slave`

默认会话名进一步附带端点，例如 `Modbus TCP Client @ 192.168.1.10:502` 或 `Modbus RTU Master @ COM5`。工作区摘要额外显示 Unit ID；串口模式还显示波特率与帧格式（例如 `9600 8N1`）。用户自定义名称始终优先，不应被自动命名覆盖。

## 设计边界

- Modbus codec/校验不进入 Transport 或 Session Runtime。
- Transport 错误必须保留结构化来源，再由 Modbus transaction 映射为协议状态。
- Client 单事务串行化是当前调试器契约；未来若开放 TCP 并发，应在 transaction engine 内显式管理 TID→request，而不是让 UI 并发 invoke 猜响应归属。
- Polling 不积压、不重入。
- 写超时必须保留 outcome unknown 语义。
- Server model 与 fault injection 属于 Modbus 模块，不做通用 Session capability。
- exact Raw ADU 不自动修正 CRC/LRC/MBAP，也不能冒充“已通过协议校验”的普通 transaction。
- 会话命名、模式/角色标签和连接摘要由 `src/plugins/modbus/model.ts` 的纯函数集中生成，避免不同页面自行拼接产生漂移。

## 代码锚点

- `src-tauri/src/plugins/modbus/mod.rs`
- `src-tauri/src/plugins/modbus/client.rs`
- `src-tauri/src/plugins/modbus/server.rs`
- `src-tauri/src/plugins/modbus/polling.rs`
- `src-tauri/src/plugins/modbus/data_model.rs`
- `src-tauri/src/plugins/modbus/config.rs`
- `src-tauri/src/plugins/modbus/codec/mod.rs`
- `src-tauri/src/transport/serial.rs`
- `src-tauri/src/transport/tcp.rs`
- `src/plugins/modbus/ModbusConnectForm.tsx`
- `src/plugins/modbus/ModbusSessionView.tsx`
- `src/plugins/modbus/Modbus.module.css`
- `src/plugins/modbus/model.ts`
- `src/plugin-manifests/modbus.json`

## 何时更新本文

修改功能码覆盖、framing/validation、重试或 broadcast 语义、Watch 调度、Server Simulator、Raw 模式、持久化边界、Modbus UI 信息架构、默认会话身份或 Modbus 与 Transport/Session Runtime 的职责关系时，必须同步更新本文。
