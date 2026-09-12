# Modbus 模块设计

## 目标

Modbus 是独立 custom Session，提供标准 Modbus RTU、ASCII、TCP 的 Client 调试、轮询监视、事务审计、Raw 调试和 Server Simulator。协议语义全部留在 Modbus 插件；串口/TCP 资源继续复用公共 Transport Runtime。

权威协议语义与实现依据见 [MODBUS 知识层](../knowledge/MODBUS.md)。本文件只描述 TauTerm 当前实现边界。

## 分层

```text
Modbus Session View
  ↓ typed Tauri commands
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
  ├─ transport capability
  ├─ request / response validation
  ├─ semantic response model
  ├─ typed value interpretation
  └─ RTU / ASCII / TCP ADU framing
  ↓
Transport Runtime
  ├─ Serial
  └─ TCP
```

Transport 不知道 Unit ID、功能码、CRC/LRC、MBAP、异常码或寄存器模型；这些全部属于 Modbus 模块。UI 只负责编辑和展示，不负责决定某个标准功能在特定传输上是否合法，也不重新解析已经由 Rust 验证过的 PDU。

Session JSON 中的 `ModbusConfig` 只是边界 DTO。连接开始后立即通过 `validated()` 转成 tagged runtime domain：endpoint 明确为 Serial 或 TCP，role 明确为 Client 或 Server。Client timeout/retry 与 Server max-clients/fault 分别只存在于对应角色语义中。

## Target 与 Unit ID

Client 的 `unit_id` 配置只是 **默认目标**，不是连接本身不可变的一部分。实际标准事务使用：

```text
ModbusOperation::Request
  ├─ unit_id
  └─ semantic ModbusRequest
```

因此同一个串口或 TCP Gateway Session 可以连续访问不同 Unit，不需要为每个 Unit 重建连接。Watch row 同样拥有自己的 `unit_id`，所以一张 Watch Table 可以在同一链路上监视多个从站。

串行 Client 的事务目标必须为 `0..=247`；Unit 0 只允许写广播且不等待响应。TCP Unit Identifier 允许完整 `u8` 范围。Server 的 Unit 仍属于 Server Session 本身，因为一个 Server 实例模拟一个目标单元。

## 标准请求、capability 与响应模型

标准 Modbus 请求使用语义化 variant，而不是“请求类型 + 任意 `function: u8`”组合，例如 `ReadBits`、`ReadRegisters`、`WriteSingleRegister`、`WriteMultipleRegisters`、`MaskWriteRegister`、`ReadWriteMultipleRegisters` 等。功能码由 variant 推导；Raw PDU / exact Raw ADU 才允许调用者显式提供任意字节。

07、08、0B、0C、11 等串行线功能的 transport capability 只在 Rust `capability` 模块维护一份。Client 和 Server 都查询同一个 capability source；UI 只显示“Serial only”提示，不复制 correctness gate。

标准响应先经过 framing、Unit/TID、function、byte-count、echo 和功能特定语义校验，再转换为 `SemanticResponse`。常用读请求直接产生 bit/register 数组；标准写产生 acknowledged 语义；高级响应至少保留经过验证后的结构化或 raw semantic payload。前端 Read/Write 和 Watch 都消费这一结果，不再从 `response_pdu` 重新实现第二套协议 decoder。原始 TX/RX/PDU 仍保留用于诊断。

## Client Transaction Engine

Client 连接时建立一个 `DataPlaneRuntime`，事务引擎保持唯一 request/response 所有权和会话级 subscription。当前同一 Client 一次只允许一个 outstanding transaction；TCP 也默认保持单 outstanding，以保持调试上下文确定性。

标准流程：

1. 校验目标 Unit 与 transport capability；
2. semantic request 编码为 PDU；
3. 按 RTU/ASCII/TCP 添加 ADU envelope；
4. 写入 DataPlane；
5. 后端 framer 收集完整响应；
6. 严格验证 Unit/TID/PID/MBAP length/function/byte-count/echo/CRC/LRC 与功能特定语义；
7. 生成 semantic response、诊断字节与 Transaction Result；
8. 加入有界 sequence history。

事务状态区分 success、broadcast、Modbus exception、protocol error、malformed response、timeout、transport error、cancelled。写请求 timeout/transport failure 保留 `write_outcome_unknown`；默认不自动重试写。读取请求可按配置重试。

TCP TID 由 Client 事务引擎拥有。迟到的旧 TID 响应被视为 stale frame，跳过后继续等待当前 TID。未来若开放 TCP 并发，必须升级为显式 TID→request Transaction Manager，而不是让 UI 并发 invoke 猜响应归属。

## ADU Framing

### RTU

RTU 帧边界只在 Rust runtime 处理。低速链路按串口字符宽度计算 t1.5/t3.5；高于 19200 baud 使用推荐固定 t1.5=750µs、t3.5=1.750ms。帧中出现超过 t1.5 但不足 t3.5 的静默间隔视为 malformed；只有完整 t3.5 边界后才进入 CRC/PDU 解析。

### ASCII

ASCII 使用独立有界 `AsciiFramer`：以 `:` 寻找帧起点，以 CRLF 结束，支持半帧和一次 read 多帧；起始符前噪声被丢弃，未完成旧帧中出现新的 `:` 时从新起点重同步，超长未终止输入会被清理。非法 hex、奇数 digit、错误 LRC、缺失终止符和超长 PDU 均不得进入正常 transaction。

### TCP

TCP 使用持续增量缓冲，根据 MBAP Length 拆分完整 ADU，支持半包、粘包和单次 read 多帧。Protocol Identifier、Length、Unit Identifier 和 Transaction Identifier 属于 Modbus protocol core，不进入通用 TCP Transport。

## 值解释

canonical address 始终为 PDU 0-based `0..65535`。传统 `00001 / 10001 / 30001 / 40001` 只属于 UI 辅助。

协议层只拥有 bit/register 原始值。应用值解释独立支持 Bool、UInt/Int16/32/64、Float32/64、Hex/Binary、ASCII/UTF-8、bit、scale/offset/unit。register 内 byte order 与多 register word order 是两个正交维度。固定宽度类型必须与请求寄存器数量一致；UInt64/Int64 跨 IPC 使用精确十进制字符串且要求 identity scale/offset。

## Watch / Monitor

Watch row 保存：稳定 id、enabled、name、目标 `unit_id`、semantic read request、period 与可选 ValueFormat。Scheduler 属于 Session Runtime，不属于 React Monitor 组件生命周期。因此切换 Read/Write、Transactions 或 Advanced 页面不会停止轮询；只有显式 Stop、断连或 Session shutdown 才停止。

Scheduler 按每行完成时刻重新安排下一次执行，不补偿错过 tick，不产生 backlog/reentry。所有行仍共享 Client 单事务引擎，因此不同 Unit 的轮询也保持确定性串行执行。

Bit 区不保存 register ValueFormat；寄存器区必须有明确 ValueFormat。固定宽度与 quantity 不一致时拒绝保存。Watch 校验直接复用 Rust request encoder 和 target Unit 规则。

HMI 使用“紧凑观察表 + 选中行 Inspector”，而不是把 16 个低/高频字段全部永久铺在一张超宽表中。表格优先展示 enabled、name、Unit、area、address、type、current value、status；quantity、byte/word order、scale、offset、engineering unit、bit、period 在 Inspector 编辑，适配 TauTerm 多分屏窄 Pane。

## Transaction History / Runtime Summary

Client/Server history 都是有界运行态 sequence log，保存 timestamp、Unit、FC、TID、attempt、latency、TX/RX/PDU、status、exception 与 outcome-unknown。历史不写入 Saved Session。

UI 不再高频读取完整 history。`modbus_status` 接受 optional sequence cursor/limit：Transactions 首次读取最近窗口，此后只取新记录；StatusBar 不带 cursor 时只取轻量最新状态。前端 Pause 只冻结事务视图，不停止协议运行时或 Watch。

## Server Simulator

四个标准地址域由统一 `AddressSpace` / `AddressBlock<T>` 管理。**模拟器管理操作** 与 **Modbus 协议写操作** 明确分开：

- 工作台 `set_*` 用于定义/初始化或管理数据点；
- protocol read/write 只能访问已经定义的点；
- FC05/06/0F/10/16/17 对未定义地址返回 Illegal Data Address，不会通过一次协议写请求凭空扩张设备地址空间；
- 多地址写先验证整个 destination，再提交，错误响应不留下部分修改；
- FC17 同时预验证 read/write ranges，再保持 write-before-read 语义。

File Record、FIFO、Device Identification 与诊断状态仍是各自高级对象模型，不硬塞进四区。

Server fault injection 动态支持 response delay、no response、forced exception。优先级为 delay → no-response → forced exception；no-response 会执行真实请求但抑制响应，forced exception 不执行真实写入。

TCP Server 支持多 client。已结束 peer worker 的 JoinHandle 会在运行期回收，不积累到 shutdown。Server 请求执行成功但 response transport write 失败时，transaction 记录为 transport error，并保留“request processed, response delivery failed”诊断；不能把“内部执行成功”冒充成“客户端一定收到成功响应”。

串口 Server 按 RTU/ASCII framing 顺序处理；合法广播写执行但绝不响应。

## Advanced / Raw

常用读写覆盖 01、02、03、04、05、06、0F、10、16、17。Advanced 覆盖 07、08、0B、0C、11、14、15、18、2B/0D、2B/0E、Raw PDU、exact Raw ADU。

标准 Advanced 功能优先以语义控件呈现，例如 Read Device Identification 直接编辑 Read Code 与 Object ID，而不是要求用户手写 `01 00`。尚未类型化的高级 payload 仍由 Rust core 验证；真正需要任意字节时进入 Raw。

Raw PDU 由 TauTerm 添加所选 transport envelope；exact Raw ADU 按用户字节原样发送，不修正 CRC/LRC/MBAP/TID/byte-count，并明确标记 response 为未验证 raw data。

## UI 与底边状态栏

Modbus 使用独立 `customView`，不显示全局 SendBar。主工作区遵循“一张 Workspace Content + 内部分区”，不再额外包一层大 `liquid-glass-card`；Read/Write、Monitor、Transactions、Advanced、Server 通过 selector 与 section divider 共享同一工作台表面。

Client Header 持续显示当前 transaction target Unit，可在连接保持不变时快速切换。连接页的 Unit 字段明确叫“默认 Unit ID”，表示新事务/Watch 的初始目标。

底边 StatusBar 由全局 UI Foundation 拥有，Modbus 只通过插件 `statusBarItems` 贡献轻量运行态：协议模式/角色、最近事务 Unit、Watch 运行计数、最近结果与 latency。Endpoint/连接状态由全局栏已有 owner 展示，Modbus 不重复标题或完整配置。Custom Modbus Session 不继承 Text/UTF-8/TX/RX stream 状态。

Modbus 插件专属中英文文案由 `src/plugins/modbus/locales.ts` 所有，经 `PluginRegistration.locales` 注入全局 i18n；协议专属文案不复制到公共 locale。

## 持久化边界

Saved Session 只保存可重建稳定配置：mode/role/endpoint、Client default Unit 或 Server Unit、timeout/retry、Server 限制/default fault、Watch definitions/value formats、Server model initial points。

不能持久化 DataPlane、subscription、TID counter、running/watch-running、poll timer、transaction history/sequence cursor、当前 request、socket/serial handle 等 runtime state。

TauTerm 处于预稳定阶段。本模块 schema 直接以当前模型为唯一事实，不维护旧 Watch schema、固定 Unit transaction schema、旧 response DTO 或其它开发期兼容映射；模型变化后旧开发配置需要重新保存/重建。

## 设计边界

- Boundary DTO 只存在于配置/持久化边界；runtime 使用 validated tagged config。
- Client Unit 是 transaction/watch target；Session config 只保存 default Unit。
- 标准 request/function、transport capability 与 response validation 的 correctness SSOT 在 Rust protocol core。
- UI 不解析 Modbus PDU；只渲染 semantic response 和 raw diagnostics。
- Client 单事务串行化是当前契约；TCP stale TID 不污染当前事务。
- RTU framing 同时遵守 t1.5/t3.5；ASCII framer 有界且可重同步；TCP 按 MBAP 增量拆帧。
- Watch 属于 Session Runtime，切页不停止；Polling 不积压、不重入。
- 写超时保留 outcome unknown。
- Transaction history 通过 sequence cursor 增量读取，不以全量轮询做运行时数据总线。
- Server 管理定义与协议写入分离；协议写只能修改已定义地址，多地址修改原子。
- Server transport delivery failure 与 request execution result 不混淆。
- exact Raw ADU 不做协议修正，也不能冒充已验证 transaction。
- Modbus 专属状态和 locale 通过 plugin contracts 注入 UI Foundation，不继续增加全局协议分支。

## 代码锚点

- `src-tauri/src/plugins/modbus/`
- `src-tauri/src/transport/serial.rs`
- `src-tauri/src/transport/tcp.rs`
- `src/plugins/modbus/`
- `src/plugin-manifests/modbus.json`
- `src/components/Layout/StatusBar.tsx`
- `src/core/plugin-registry.ts`

## 何时更新本文

修改 runtime config/target model、request/response schema、功能 capability、framing/validation、retry/broadcast/TID、Watch 生命周期或值布局、transaction history transport、Server Address Space/fault/delivery semantics、Raw 模式、持久化边界、Modbus HMI、StatusBar 贡献或 Modbus 与 Transport/Session/UI Foundation 的职责关系时，必须同步更新本文。
