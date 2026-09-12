# Modbus 模块设计

## 目标

Modbus 是独立 custom Session，提供标准 Modbus RTU、ASCII、TCP 的 Client 调试、轮询监视、事务审计、Raw 调试和 Server Simulator。协议语义全部留在 Modbus 插件；串口/TCP 资源继续复用公共 Transport Runtime。

权威协议语义与实现依据见 [MODBUS 知识层](../knowledge/MODBUS.md)。本文件只描述 TauTerm 当前实现边界。

## 分层

```text
Modbus Session View
  ↓ Tauri commands
Boundary DTO / session persistence
  ↓ validated()
Validated Modbus runtime domain
  ├─ endpoint = Serial(RTU/ASCII) | TCP
  └─ role = Client | Server
  ↓
Modbus runtime
  ├─ Client transaction engine
  ├─ Watch scheduler
  └─ Server simulator
  ↓
Protocol core
  ├─ semantic request model
  ├─ request / response validation
  ├─ typed value interpretation
  └─ RTU / ASCII / TCP ADU framing
  ↓
Transport Runtime
  ├─ Serial
  └─ TCP
```

Transport 不知道 Unit ID、功能码、CRC/LRC、MBAP、异常码或寄存器模型；这些全部属于 Modbus 模块。UI 只负责编辑和展示，不负责决定某个标准功能在特定传输上是否合法。

Session JSON 中的 `ModbusConfig` 只是边界 DTO。连接开始后必须立即通过 `validated()` 转换为 tagged runtime domain：endpoint 明确为 Serial 或 TCP，role 明确为 Client 或 Server；Client timeout/retry 与 Server max-clients/fault 分别只存在于对应运行角色语义中。运行时不继续携带一组“所有模式都可见、但大部分字段无效”的平铺配置作为核心状态。

## 标准请求模型

标准 Modbus 请求使用语义化 variant，而不是“请求类型 + 任意 `function: u8`”组合。例如：

- `ReadBits { area = Coils | DiscreteInputs }`；
- `ReadRegisters { area = HoldingRegisters | InputRegisters }`；
- `WriteSingleCoil { value: bool }`；
- `WriteSingleRegister { value: u16 }`；
- `WriteMultipleCoils { values: Vec<bool> }`；
- 其它标准功能也分别拥有明确 variant。

功能码由 variant 推导，因此标准 API 在类型层不能构造“Read Registers 但 function=0x01”这类非法组合。只有 Raw PDU / exact Raw ADU 继续允许调用者显式指定任意字节，因为 Raw 的职责就是协议逃生舱和畸形帧测试。

前端 TypeScript 类型镜像该 IPC schema，但协议合法性、数量上限、功能 capability、响应语义的唯一裁决点在 Rust protocol core。前端显示 label、提示和输入 datatype 边界，不复制一套决定请求是否符合 Modbus 标准的验证器。

## Client

Client 会话在连接时建立一个 `DataPlaneRuntime`，事务引擎对它保持唯一 request/response 所有权，并保持一个会话级 `DataPlaneSubscription`。当前同一 Client 一次只允许一个 outstanding transaction：RTU/ASCII 由协议顺序性要求如此，TCP 调试器也保持默认 1 outstanding，以避免调试上下文被并发响应打散。

标准执行流程：

1. 请求编码为 PDU；
2. 根据 RTU/ASCII/TCP 添加 ADU envelope；
3. 写入公共 DataPlane；
4. 按 transport/framing 规则组帧；
5. 严格校验 Unit/TID/PID/MBAP length/function/byte-count/echo/CRC/LRC，以及功能特定响应结构；
6. 输出结构化事务结果并加入有界历史。

事务状态明确区分 success、broadcast、Modbus exception、protocol error、malformed response、timeout、transport error 和 cancelled。写请求超时或 transport failure 时保留 outcome unknown 语义，不能把“未收到响应”解释为“设备一定没有执行写入”。

读取请求可以按配置重试；写请求默认不重试，只有显式启用时才复用 retry 次数。串口 Unit 0 只允许 write broadcast，发送后不等待响应。

TCP Transaction Identifier 在 Client 内由事务引擎拥有。重试后迟到的旧 TID 响应属于 stale response：在当前单 outstanding 模型下跳过并继续等待当前 TID，不能把旧响应误判为当前事务协议错误。未来若开放 TCP 并发，必须把 framing 和 TID→request dispatch 升级为显式连接级 Transaction Manager，而不是让 UI 并发 invoke 猜响应归属。

Subscription 生命周期与 DataPlaneRuntime 生命周期绑定：Client shutdown 时先释放 subscription，再 join runtime；不能反复 subscribe 丢失接收所有权，也不能让 subscription guard 在 runtime join 之后继续存活。

## ADU framing

### RTU

RTU 帧边界由后端协议运行时处理，不依赖 WebView timer。低速链路按串口字符宽度计算 t1.5 和 t3.5；高于 19200 baud 时使用规范推荐的固定 t1.5=750µs、t3.5=1.750ms。已经开始接收的帧若在完成前出现超过 t1.5 但不足 t3.5 的静默间隔，视为畸形帧并丢弃；只有达到完整 t3.5 静默边界后才进行 CRC/PDU 解析。

### ASCII

ASCII 由 `:`、十六进制字符、LRC 和 CRLF 界定。非法字符、奇数 hex digit、缺失 CRLF、错误 LRC 和超长 PDU 均不得进入正常事务结果。

### TCP

TCP 使用持续增量缓冲，根据 MBAP Length 拆分完整 ADU，支持半包、粘包和单次 read 包含多帧。Protocol Identifier、Length、Unit Identifier 与 Transaction Identifier 都属于协议校验，不进入通用 TCP Transport。

## 功能码与 capability

常用读写界面覆盖 01、02、03、04、05、06、0F、10、16、17。

Advanced 入口覆盖串口诊断和高级功能：07、08、0B、0C、11、14、15、18、2B/0D、2B/0E，以及 Raw PDU / exact Raw ADU。

07、08、0B、0C、11 属串行线标准功能。UI 只把这些入口标记为“串行线专用”，不自行维护一套 TCP capability gate；最终裁决在 Rust 协议核心：TCP Client 不发送这些标准请求，TCP Server 收到后返回 Illegal Function。Raw PDU / Raw ADU 不受此标准功能 capability 限制，因为其用途就是显式构造非标准或畸形报文。

Diagnostics 当前明确支持：

- `0x0000` Return Query Data；
- `0x000A` Clear Counters and Diagnostic Register（Data 必须为 `0x0000`，响应回显该 Data）。

其它 Diagnostics sub-function 在未实现完整语义前不得做“透明成功回显”，而应返回标准非法数据值。通信事件计数器只在成功处理且标准语义要求计数的消息后增加；读取事件计数器本身和清零诊断不自增。

Response validator 不只检查“功能码相同/长度大致正确”。当前高级语义还包括：

- FC14 Read File Record：每个 sub-response 必须与对应请求 record length 和 reference type 对齐；
- FC0B/0C：communication status 必须是标准状态值，event log byte count 必须自洽；
- FC11：Server ID payload 必须至少包含 server id 与合法 run indicator；
- FC18：FIFO byte-count 与 FIFO-count 必须互相一致且数量不超过标准上限；
- FC2B/0E：read code、conformity、more-follows、对象数量、对象边界、对象 ID 顺序和 specific-object 响应都进行结构/语义校验。

Raw PDU 仍由 TauTerm 添加所选模式的 envelope；exact Raw ADU 则按用户提供字节原样发送，不做协议修正，并明确标记响应为未验证 raw 数据。

## 地址与值解释

内部 canonical 地址始终为 0..65535 的 PDU address。传统 `00001 / 10001 / 30001 / 40001` 只属于显示/输入辅助，不能进入底层协议状态。

协议层只拥有 bit/register 原始数据。应用值解释是独立层：

- Coils / Discrete Inputs 直接解释为 bit/bit array，不经过 register decoder；
- Holding/Input Registers 可以解释为 Bool、UInt/Int16/32/64、Float32/64、Hex/Binary、ASCII/UTF-8、bit、scale/offset/unit；
- register 内 byte order 与 multi-register word order 是两个正交配置，不把 ABCD/BADC/CDAB/DCBA 当成底层唯一数据模型；
- 固定宽度类型决定需要的寄存器数量，例如 16-bit=1、32-bit=2、64-bit=4；Watch 不允许“请求 125 个寄存器但只解释前 2/4 个寄存器”的隐式歧义；
- UInt64/Int64 跨 IPC 使用精确十进制字符串表示，不经 JavaScript `Number` / Rust `f64`，因此不丢失 53-bit 以上整数精度。为了保持精确整数语义，这两种类型不允许非恒等 scale/offset。

这些值布局是工程数据约定，不改变线上 Modbus 字节序语义。

## Watch / Monitor

Watch row 保存稳定定义：请求、周期、值格式和显示名称。Scheduler 对每行按完成时刻重新安排下一次执行，不补偿错过的 tick，因此不会因慢设备产生 backlog/reentry。停止会话或显式 Stop 会终止轮询；运行态 value、latency、timestamp 不作为持久化配置。

Bit 区 Watch 不保存寄存器 ValueFormat。寄存器区必须具有明确 ValueFormat；固定宽度格式与 quantity 不一致时拒绝保存，而不是静默截断。Watch 保存校验直接复用 Rust protocol core 的请求编码/范围校验，因此数量上限等标准规则不在 Monitor UI 或 Scheduler 中复制第二份。

当前 Scheduler 仍保持单 Client 顺序事务语义，这是调试器默认行为。若未来增加大点表优化，应把“用户定义的事务”与“可选的连续地址合并 Planner”分开，默认 Exact Polling 不应偷偷改变请求边界。

## Transaction History

Client/Server 历史都是有界运行态记录，保存时间戳、Unit、FC、TID、attempt、latency、TX/RX/PDU、status、exception 和 outcome-unknown 标记。事务历史不写入 Saved Session。

## Server Simulator

Server 的四个标准地址域由统一 `AddressSpace` 管理，每个区使用 `AddressBlock<T>`：

- Coils；
- Discrete Inputs；
- Holding Registers；
- Input Registers。

`AddressBlock` 统一负责 set/get/read/write/snapshot/range validation，避免四套 Map helper 漂移。File Record、FIFO、Device Identification 和诊断状态属于各自高级对象模型，不硬塞进标准四区。

所有可能跨多个地址的写操作必须先验证完整目标范围，再提交修改；如果请求最终返回异常，不能留下“前半段已经写入”的部分状态。FC17 在读写范围重叠时仍保持标准的 write-before-read 结果，但在提交写入前先确认最终读范围可满足，因此错误响应不会伴随部分写入。

Device Identification 的 individual access 只返回所请求对象，使用支持 individual access 的 conformity level；不存在的对象返回 Illegal Data Address。stream access 遇到未知 Object ID 时按规范从 Object 0 重新开始，并按对象 ID 有序返回、正确生成 more-follows / next-object 元数据。

Server fault injection 可动态设置：

- response delay；
- no response；
- forced Modbus exception。

优先级固定为：先应用 delay；若启用 no response，则执行真实请求但抑制响应，以模拟客户端无法确认写结果的真实故障；只有未启用 no response 时才应用 forced exception，且 forced exception 不执行真实写入。Fault exception code 只接受标准 Modbus exception 集合，不接受保留值。

TCP Server 支持多 client；串口 Server 按 RTU/ASCII framing 顺序处理请求。串口广播执行合法写操作但绝不发送响应。

## 持久化边界

Saved Session 只保存可重建的稳定配置，例如 mode/role、endpoint、Unit ID、timeout/retry、Server 限制与默认 fault、Watch 定义和值格式、Server model 初值。

不能持久化 DataPlane、subscription/receiver、TID 计数器、running flag、poll timer、transaction history、当前请求、socket/serial handle 等运行态。

TauTerm 当前处于预稳定阶段。本模块内部 schema 直接以当前模型为唯一事实，不维护旧 ModbusRequest、ValueFormat 或 Watch schema 的兼容映射；旧开发期保存配置在模型变化后需要重新保存/重建。

## UI

Modbus 使用独立 `customView`，不显示全局 SendBar。新建会话入口统一显示 `Modbus 调试助手`，默认传输模式为 RTU；后端在缺少自定义名称时同样使用这一名称作为兜底，避免不同连接路径生成不同默认名。

工作区采用“一张主工作台 + 内部分区”的信息架构，不把请求、结果、数据模型等区域各自包装成独立悬浮玻璃卡片：

- Read / Write：请求编辑器 + 结构化结果 + Raw diagnostics；
- Monitor：可编辑 Watch Table；
- Transactions：时间序列事务表；
- Advanced：诊断、文件记录、设备标识与 Raw 操作；
- Server：模拟数据模型与运行时状态。

这套 UI 是调试工作流设计，不是 Modbus 协议标准规定的 HMI。主题材质继续遵循全局 Liquid Glass SSOT，本模块不另建第二套玻璃体系。

### 会话身份

会话卡片的协议身份采用两层信息：第一行是 `Modbus @ RTU Master` / `ASCII Slave` / `TCP Client` / `TCP Server`；第二行是串口或 IP:Port。用户显式输入的自定义会话名始终优先。

会话展示字符串集中在 `src/plugins/modbus/presentation.ts`，协议模型与 UI 文案分离。

## 设计边界

- Boundary DTO 只存在于配置/持久化边界；运行时只使用 validated tagged config。
- 标准请求由语义 variant 推导功能码，不允许裸 `function: u8` 与请求类型组成非法状态。
- Codec/validation 不进入 Transport 或 Session Runtime。
- Transport 错误保留结构化来源，再由 transaction engine 映射为协议状态。
- 标准 capability 和协议范围由 Rust 协议核心最终裁决，前端不维护第二套 correctness validator。
- Client 单事务串行化是当前默认契约；TCP stale TID 不能污染当前事务。
- RTU framing 同时遵守 t1.5 与 t3.5。
- Polling 不积压、不重入。
- 写超时必须保留 outcome unknown。
- Server 标准地址域由统一 AddressSpace/AddressBlock 抽象管理，多地址修改必须是事务原子的。
- Bit area 与 register value codec 必须分离。
- advanced response 不只做表层长度检查，必须按各功能语义校验嵌套结构。
- exact Raw ADU 不自动修正 CRC/LRC/MBAP，也不能冒充“已通过协议校验”的普通 transaction。
- UI 展示身份由 presentation 层生成，不允许多个组件各自拼接不同格式。

## 代码锚点

- `src-tauri/src/plugins/modbus/`
- `src-tauri/src/transport/serial.rs`
- `src-tauri/src/transport/tcp.rs`
- `src/plugins/modbus/`
- `src/plugin-manifests/modbus.json`

## 何时更新本文

修改运行时配置域模型、标准请求 schema、功能码覆盖、framing/validation、重试/broadcast/TID 语义、Watch 调度或值布局、Server Simulator、Raw 模式、持久化边界、Modbus UI 信息架构、默认会话身份或 Modbus 与 Transport/Session Runtime 的职责关系时，必须同步更新本文。
