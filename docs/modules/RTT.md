# RTT 调试助手

## 目标

RTT 调试助手为嵌入式目标提供长期运行的 Real Time Transfer 会话，负责调试探针连接、RTT Control Block 定位、多 Up/Down Channel 收发、有限历史、Session Data Log，以及原始 Terminal/Log/HEX 与语义化 SystemView/RTOS Trace 观察工作区。

RTT 是多通道目标内存通信机制，不等价于单个串口字节流。本模块使用 **Container Session + 插件私有 Runtime**，不把某个 RTT Channel 伪装成根 Session `DataPlane`；公共 Kernel 不解释 probe、Control Block 或 RTT Channel 语义。

## 当前方案

```text
Saved RTT Session
        │
        ▼
SessionStore / Container Session
        │
        ├── AutomationIo ── shared SendBar / Lua / Auto Reply
        │
        ▼
     RttRuntime
        │
        ▼
 bounded RTT worker
   ┌───────────────┴────────────────────────┐
   │                                        │
Native debug-probe backend          Existing J-Link backend
   │                                        │
EmbeddedDebugManager                127.0.0.1 RTT TELNET
   │
shared DebugTargetRuntime worker
   │
probe-rs Session / short Core borrow
   │
RTT service lease + RTT state
   └──────── canonical RTT frames ──────────┘
                    │
       ┌────────────┼───────────────┬────────────────┐
       ▼            ▼               ▼                ▼
 bounded history  LogEngine   presentation batch  semantic observers
                                  │                │
                              runtime store        └─ SystemView decoder
                                  │                     │
                         Terminal / Log / HEX      RTOS Trace / Events / Raw
```

`RttRuntime` 通过 `SessionService` 挂到 Container Session，并由 RTT 插件自己的 `SessionRuntimeRegistry<RttRuntime>` 建立弱索引。SessionStore 仍是用户可见连接生命周期的唯一权威所有者；RTT Runtime 只持有插件私有资源与状态。

`EmbeddedDebugManager` 是进程内的物理探针所有权 registry。进入 registry 前，Auto/显式 selector 都先解析为 canonical selector；registry 以 canonical physical probe identity 为槽位，同一探针只有一个活动的 `DebugTargetRuntime`。相同 target/wire/speed 配置复用该 runtime；同一物理探针若请求不同目标配置则显式返回冲突，而不是尝试第二次打开 USB probe。Probe 在 startup timeout 后若底层打开线程仍未退出，slot 保持 `Opening`；shutdown 超过边界而 detach 的旧 worker 也继续保留物理 Probe reservation，直到其真正退出，因此弱 Runtime 失效本身不等价于 Probe 已可重新打开。每个 `DebugTargetRuntime` 都由单独 worker 线程唯一拥有 probe-rs `Session`，RTT worker 只通过有界调度队列提交短操作，`Core` 仍只在该 worker 中短生命周期借用。RTT 取得独占的 `rtt` service lease，防止同一物理目标出现第二个 RTT reader；未来变量采样等不同 service 可以复用同一 target worker，而不复制 probe handle。共享 target scheduler 为每个 service 建立独立有界队列，并按 service round-robin 取短操作；单个高频 observation service 不能占满整个 target queue 或持续饿死其它 service。

## Backend

### 原生调试探针

原生 backend 使用锁定版本的 `probe-rs`。probe 枚举和连接身份仍由公共 embedded-debug 层提供，RTT 模块只拥有 Control Block 定位、Channel metadata、Up/Down I/O 与 RTT 错误语义。配置支持：

- 自动选择唯一探针或显式 probe selector；
- 目标芯片；
- SWD / JTAG；
- 自动或显式接口速度；
- CPU Core index；
- 可选 ELF/AXF 固件符号文件；
- 自动、精确地址或显式范围三种 RTT 定位模式。

自动定位若配置了固件符号文件，先从文件中的 `_SEGGER_RTT` 符号取得 Control Block 地址；文件可解析但没有该符号时回退目标 RAM 扫描。文件不存在或格式不可解析属于独立配置/固件错误，不静默伪装成目标 RAM 扫描失败。

attach timeout、poll cadence、write timeout 属于 Runtime 调度策略，不是 Saved Session 的用户参数。接口速度留空才表示 probe 默认速度，不使用 `0` 作为 UI 哨兵值。

Channel 刷新通过共享 target worker 使用当前 Session/Core 与原定位策略重新 attach RTT，成功后原子替换 RTT handle 与 Channel metadata；失败时保留原 runtime。RTT poll/write/refresh 都不能直接取得 probe-rs Session 所有权。

原生 backend 保留 probe-rs 的严格 attach 作为健康目标的首选路径。若严格 attach 仅因单个 Channel descriptor 损坏而失败，TauTerm 会在同一已定位 Control Block 上做逐方向验证：未使用且 `pBuffer == 0` 的 descriptor 正常忽略；buffer/size/offset/flags 不合法的方向被隔离并保留诊断信息；只要仍存在至少一个可安全读写的方向，会话以 degraded 状态继续运行。RTT magic、Channel 数量或 Control Block 基础布局无效，或所有方向都不可用时仍整体拒绝。degraded 路径只对已验证方向执行 ring-buffer I/O，不修改目标 descriptor，也不通过 RAM 扫描掩盖错误。

### 已有 J-Link 调试会话

兼容 backend 只连接 `127.0.0.1` 上现有 J-Link RTT TELNET 服务，用于与已经占用 J-Link 的 IDE/Debugger 共存。它不会打开 USB probe，也不会泛化成任意远程 TCP RTT 客户端。

该 backend 按显式 Channel 列表建立 loopback 连接，并遵循 SEGGER RTT TELNET Channel 选择协议。它只报告真实可用能力，不声称支持 probe 枚举、Control Block 定位、完整 Channel metadata 或直接目标控制。

## 连接诊断与系统日志

RTT 的连接建立不是黑盒操作。System Log 必须记录足够的结构化阶段信息，使现场问题能够区分“探针/目标 attach”“RTT 定位”“Control Block/Channel 校验”和“运行期 I/O”：

- 启动阶段记录 Session、backend、probe selector、target、wire、speed/core、固件符号文件与定位策略；
- Native backend 明确记录定位来源：ELF/AXF 的 `_SEGGER_RTT`、用户精确地址、用户范围或 RAM 扫描回退；ELF 符号命中时同时记录解析出的 Control Block 地址；
- 成功 attach 后记录实际 Control Block 地址、Channel 数量和各 Channel 的 Up/Down buffer size 摘要；
- startup、Channel refresh 与运行期 fatal error 都以稳定的 RTT error code + message 进入 System Log；底层库技术详情可以保留，但用户提示必须先给出 TauTerm 语义；
- Control Block 被判定损坏时，Native backend 额外抓取一个有界 metadata 快照：Control Block header、Up/Down descriptor 的地址/缓冲地址/大小/读写偏移/flags，并比较 descriptor 静态字段（名称指针、缓冲地址、大小）的批量 32-bit 读取与逐 word 读取结果；读写偏移属于运行期可变字段，不参与一致性判定。该快照用于区分“某个未使用/跟踪 Channel descriptor 异常”和“CMSIS-DAP 批量内存读取不一致”，不包含 RTT payload；
- ELF/AXF 符号或用户 Exact 地址已经把定位收敛为单一地址、但 attach 仍返回 Control Block Not Found 时，再抓取该地址前 16 bytes 的只读证据：byte read、批量 32-bit read、逐 word 32-bit read、各自 RTT magic 匹配结果以及跨读取方式一致性。该诊断只验证 Control Block magic，不扫描其它 RAM，也不把 Exact 失败静默回退成 RAM 扫描，避免掩盖底层 memory-access 兼容问题；
- 诊断日志不记录 RTT payload，也不把 Session Data Log 的数据内容复制到 System Log。

连接失败仍保持“backend 完成 attach/定位并取得初始 Channel metadata 后才发布 Connected”的边界。补充诊断日志不能改变连接成功语义，也不能用日志副作用掩盖真实错误。

## Channel、发送与自动化

RTT Up 与 Down 是独立方向。同一 index 可以仅 Up、仅 Down，或同时具有 Up/Down。

工作区中的当前观察 Channel、Automation receive source 与 SendBar 的发送 Channel 是三个独立状态。切换观察 Channel 只改变 viewer，不再隐式改写正在使用或下一次订阅使用的 Automation receive source。无效方向仍在 Channel rail 中显示为诊断项，但不会成为观察源、Automation receive source 或发送目标：

- **View Channel**：只负责当前工作区展示；有 Up 时可进入原始 Terminal/Log/HEX，语义观察器存在时进入对应高级视图；
- **Automation receive source**：必须有 Up，由 Rust Runtime 独立维护；Auto Reply/Lua 订阅创建时固定该 source，不随之后的 View Channel 切换。普通 RTT 工作区仅在存在多个可选 Up Channel 时于公共 TargetBar 暴露该选择，SystemView 等语义观察器不作为普通文本自动化来源；
- **Send target**：必须有 Down，由公共 SendBar 顶部 `RttSendTarget` 选择；
- Terminal 模式的键盘输入直接写当前 Terminal 对应的 Down Channel，这是终端交互，不是第二套发送栏。

上层双向协议可以声明 **Down Channel claim**。被 SystemView 等协议观察器占用的 Down Channel 不再进入公共 SendBar/Automation 通用发送目标，也拒绝普通 `rtt_write`，避免文本或脚本数据污染协议控制流；协议自身通过 Runtime 内部控制路径写入。

RTT 启用 TauTerm 公共 SendBar。Basic/Command/Auto Reply/Script 继续使用统一 SendBar 产品体验；底层通过协议无关 `AutomationIo` 接入，而不是要求 RTT 根 Session 伪造 `DataPlane`。Lua 的 `send()` 使用当前 Down target，`send_to("rtt:<index>", ...)` 可显式指定 Channel；接收订阅来自当前 Up source。Send target 行是否存在由插件运行态统一判定：只有实际存在可用 Down Channel 时才渲染并计入 SendBar 高度，断连、连接失败或没有可写方向时不预留隐藏目标栏空间。当前 View 进入 SystemView RTOS Trace / Events / Raw 等语义观察器时，插件通过公共运行态 presentation contribution 隐藏 SendBar surface，但保持对应 Session 的 SendBar provider 挂载，切回普通 RTT 视图时输入草稿和自动化执行所有权不会因 UI 隐藏而丢失。

重连创建新 runtime generation 后，前端会重新同步当前 Up source 与 Down target，不能让前端保留选择和新 backend 默认值发生隐式漂移。

## 原始数据模型与调度

RTT 数据离开 backend 的当刻就形成 canonical frame，包含：

- runtime generation；
- 全会话单调 sequence；
- host acquisition timestamp；
- Channel index；
- 该 Channel 的单调 byte offset；
- 原始 payload。

sequence/offset 不在 WebView presentation 阶段补造，因此不同 Channel 的原始到达顺序和每 Channel 偏移不会因批处理而丢失。

worker 每个 tick 只处理有界数量的控制命令；Down 写入按固定 byte quantum 轮转推进，不能让一个满缓冲 Down Channel 在整个 write timeout 内独占 worker。Native 与 Existing J-Link backend 的一次 Up poll 都限制 Channel 数与每 Channel read 次数，并以轮转 cursor 推进，避免任一 backend 形成无界长操作。共享 DebugTarget scheduler 再按 service 独立队列 round-robin 调度。Queue full、排队 deadline 与“操作已 dispatch 但等待结果超时”分别保留为 scheduler pressure / operation timeout / outcome unknown，不伪装成 Probe 物理断开；只有实际 probe/core I/O 失效才触发 fatal disconnect。RTT poll 若持续读到数据，会在严格有界的 busy-drain burst 内立即继续轮询，并在每轮之间照常处理控制命令与 Down 写入；读空或达到 burst 边界后才进入常规 poll sleep。这样高吞吐 SystemView/日志流不必每读一轮就固定等待，同时也不会用无限 busy loop 饿死发送和其它调试服务。

## 历史、日志与丢失语义

同一 canonical frame 分别进入三个用途不同的路径：

1. **bounded history**：进程内切回 Session/重建视图时回放；
2. **Session Data Log**：用户显式开启时进入公共 LogEngine，记录为 `[RX][RTT:n]` / `[TX][RTT:n]`；
3. **presentation batch**：短周期批量发送 WebView。

历史缓存具有 per-channel 与 per-session 总预算；超限只淘汰最老历史并累计 history eviction，这属于回放保留窗口前移，不属于采集丢失，因此不作为持续黄色告警展示。AutomationRx 队列过载只累计 automation loss；SystemView presentation queue 过载会累计历史 queue-drop 计数，但前端检测到 sequence gap 或计数推进后必须从 Rust semantic history 回补当前有界窗口，并单独维护“待回补”与“已回补”状态。已经成功回补的 presentation queue-drop 不再继续把当前 Trace 标成 presentation-incomplete。两者都不能被描述为原始 RTT 丢失或日志丢失。Session Data Log 自身的队列/磁盘损失继续由 LogEngine 健康状态负责。

canonical RTT frame 在采集时发布到共享的 typed bounded `ObservationSource<StoredRttChunk>`。AutomationRx 与当前 SystemView decoder 都直接订阅该 canonical source；任何 defmt/自定义 telemetry decoder 也必须复用同一 source，不允许创建第二个 RTT reader。每个 subscriber 使用独立有界队列和 drop hook，慢消费者只影响自己的 delivery，并可把自己的 loss 计入对应语义。目标端 RTT/SystemView overflow、host acquisition loss、history eviction、decoder subscriber loss、automation loss、recording loss、presentation loss 必须保持区分。

## 前端运行态与后台生命周期

RTT 的 snapshot、generation、Channel buffer、当前观察 Channel、view mode 与 SystemView presentation cache 由插件级 runtime store 持有，不散落在 `RttSessionView` 的临时 React state 中；Automation source、Send target、semantic observer 与 Channel claim 则由 Rust Runtime snapshot 作为唯一权威状态，前端只镜像后端确认后的选择，不能自行维护第二份发送目标真值。切换 Session/Pane 不停止 worker；切回来仍看到同一进程内 runtime 的状态与有限历史。前端缓存同时执行 per-channel 与 per-session 总预算，Log 视图只挂载有界的近期 chunk，避免多 Channel 长时运行把 WebView 内存和 DOM 数量按 Channel 数线性放大。

同一 Saved Session 重连会创建新的 generation；新 generation 到达时清空旧 presentation cache 并重新同步 source/target，禁止旧事件污染新 runtime。

## 生命周期

连接成功边界是 backend 已打开、RTT 已完成 attach/定位并取得初始 Channel metadata；此前公共 Session 不发布 Connected。

正常关闭按“停止接受新工作 → 请求 RTT worker shutdown → flush presentation → backend shutdown → runtime registry detach”收敛。共享 debug-target worker 使用有界 shutdown handshake：正常返回时 join；若底层 probe I/O 卡死超过边界，则断开命令队列并放弃阻塞等待，让 worker 在底层调用最终返回后自行退出，不能因为不可取消的 USB/debug 调用把应用关闭永久卡死。运行期 probe 拔出、目标掉电或 backend fatal error 会把 runtime 置为 Faulted，并通过 SessionStore 统一标记 Session 断开；异常断开可使用公共 `retain_terminal` 保留当前进程内只读现场。

## UI

Saved Session 遵循统一两行 presentation：第一行默认名称只在创建时生成，后续配置修改不自动改名；第二行动态显示 probe 简名或 `127.0.0.1:<port>`。

连接页使用统一控件高度/矩阵布局。常用项只展示连接方式、probe、target、wire protocol、固件符号文件和 RTT 定位；speed/core 收入高级设置。工作区复用 Workspace Content surface，不再拥有第二套发送框或额外玻璃 Card。

宽 Pane 使用左侧 Channel rail + viewer；窄 Pane 使用 `session-pane` container query 把 Channel 列表调整为顶部横向区域。普通 RTT Channel 使用 Terminal / Log / HEX；SystemView 观察器 Channel 使用 RTOS Trace / Events / Raw，Raw 只保留原始二进制诊断，不再把 SystemView payload 当 UTF-8 日志渲染。RTT Terminal 与公共 Terminal renderer 共用 `xtermLifecycle`：只在宿主已连接且可测量时 open/fit，并在 renderer dispose 前取消 ResizeObserver/RAF，协议视图不得再私建一套不安全的 xterm 生命周期。语义观察器视图不显示普通 SendBar；Trace 的 START/STOP 等协议动作由 observer 自己的紧凑工具栏负责。RTOS Trace 主视图包含采集控制、分层 loss、timestamp clock、CPU clock、任务时间线和任务统计；时间线使用目标端 SystemView timestamp delta，并在已知 SysFreq 时换算成相对时间，不把 host acquisition timestamp 或 CPUFreq 冒充时间戳时钟。时间线从有界事件历史中先重建可见窗口左边界的 Task/Idle/ISR 状态，再绘制窗口内区间，ISR 进入/退出会正确截断并恢复 Idle；Task terminate 也会结束对应活动区间。任务 lane 优先保留最近活跃/高运行时间任务，统计表按 runtime 排序；高 DPI Canvas 依据真实 Pane 尺寸重建 backing store，时间刻度按真实文本宽度夹紧到画布边界。任务表 header 与滚动 rows 分层，不用透明 sticky header 覆盖正文。存在目标端/decoder 缺口时，任务占比以“至少”语义展示；若目标端报告的丢失事件已经多于成功观察事件，则隐藏伪精确百分比，只保留已观察运行时间下界。

## SystemView / RTOS Trace

SystemView 是 RTT 上层语义观察器，不属于 RTT backend。Runtime 根据 Channel metadata 中的 `SysView` / `SystemView` 名称自动挂载，也提供显式 attach 命令用于 metadata 不完整或自定义命名场景。挂载 observer 本身是被动行为，不自动向目标发送 START；开始/停止 Trace 必须由用户显式操作。开始采集后 Runtime 自动请求 System Description / Task List / System Time。之后若 decoder 已经观察到 Task ID、但名称或优先级仍缺失，**Rust SystemView Runtime** 按未知任务集合维护自己的元数据恢复状态并发送仅包含 GET_TASKLIST 的有界退避重试：当前策略最多 10 轮，覆盖约数分钟的慢响应窗口；未知任务集合一旦取得部分进展会重新开始一轮退避，全部识别后立即停止。该恢复过程属于语义 observer 生命周期，不依赖 React 组件是否挂载、Pane 是否切走，也不会永久高频轮询；前端只镜像“同步中 / 已耗尽 / 无控制通道”的真实状态。每个 SystemView observer：

- 只订阅指定 RTT Up Channel 的 canonical `StoredRttChunk`，不存在第二个 probe/RTT reader；
- 使用 Rust streaming decoder 处理跨 chunk 半包、变长整数、标准/长度包、同步前缀与 target timestamp delta；
- 解码 Trace Start/Stop、Overflow、ISR、Task Create/Info/Run/Ready、Idle、Timer、System Description、Init、Marker 等基础事件，并保留未知/用户事件；
- 维护有界事件历史、任务运行周期与切换次数，并保留足够的前序事件上下文供 WebView 重建当前时间窗入口状态，再以批量事件和 snapshot 发送 WebView；
- Task terminate 会结束对应活动任务，并把该 Task ID 标记为已终止；若之后收到同 ID 的 Task Create，则作为新的任务实例重置旧名称、优先级和累计运行统计，避免 RTOS 复用任务地址/句柄时串接旧状态；
- 支持同一 RTT Session 同时存在多个 observer，因此数据模型不把 SystemView 写死为“唯一 Channel”，可自然扩展到多核/多 trace source；
- 多 observer 场景只选择一个可用的同索引 Down Channel 作为共享 SystemView controller，并只对该通道声明 `SystemView` claim；所有 observer 的 START/STOP/GET_SYSDESC/GET_TASKLIST/GET_SYSTIME 都经该内部控制路径发送，其余 observer 保持纯 Up 被动语义，避免无谓占用无关 Down Channel；完全没有可用 Down 时仍可被动解析已经在运行的 trace；
- 目标端 Overflow 事件、decoder subscriber drop、decoder parse error 与 WebView presentation drop 分开统计；target overflow 发生时把 overflow packet 的 delta 区间视为未知，不归属给此前运行任务；subscriber drop 会清空半包 decoder 状态并重新等待下一次 10-byte sync marker，禁止在缺口后继续猜包边界；
- 前端对目标端/decoder/尚未回补的 presentation 缺口显示 Trace incomplete 状态。目标端 Overflow 同时展示累计丢失、最近丢失速率和“已观察事件 /（已观察 + 目标报告丢失）”覆盖估算，并在诊断提示中带出当前 RTT Up Buffer 大小，帮助区分“目标仍持续溢出”与“过去曾溢出”。decoder subscriber drop 与 decode error 独立显示；presentation queue drop 成功从 semantic history 回补后保留历史诊断但退出当前缺口状态。可定位的 overflow 区间作为未知时间带展示。缺口发生后不继续把未知执行时间归属给此前任务，也不把缺失事件伪装成连续任务执行或精确 CPU 占比。

前端 Canvas 时间线只消费已经解码的 target-time event model；React 不承担二进制协议解码，也不为每个 trace event 创建时间线 DOM。Events 视图仅挂载有界近期事件，Raw 视图继续读取原 RTT 历史用于协议诊断。

## 扩展边界

SuperWatch/实时变量和 SystemView/RTOS Trace 共享“probe/target 所有权、固件符号、调度、观测/loss 模型”，但数据源不同。native probe 所有权已经提升到共享 Embedded Debug runtime：RTT 只是其中一个 service，不能再创建插件私有 probe worker。

直接内存采样必须通过同一个 `EmbeddedDebugManager` 获取 target worker，并使用独立 service lease/公平的短操作调度；SystemView/defmt/自定义 RTT telemetry 则订阅 RTT canonical raw source，不允许建立第二个 RTT reader。变量/DWARF/SVD 解析、采样策略和 Trace decoder 都留在各自 observation domain，不能塞进 RTT backend 或公共 Kernel。

## 代码锚点

- `src/plugin-manifests/rtt.json`
- `src-tauri/src/embedded_debug/`
- `src-tauri/src/plugins/rtt/`
- `src/plugins/rtt/`
- `src-tauri/src/session/io.rs`
- `src-tauri/src/kernel/log_engine.rs`
- `src-tauri/src/kernel/log_writer.rs`
- `src/components/SendBar/`

## 何时更新本文

修改 RTT backend、probe 所有权、固件符号/Control Block 定位、Channel/source/target 模型、canonical frame、调度、历史/丢失语义、Session Data Log、后台生命周期或工作区时必须同步更新本文。

外部语义与上游接口依据见 [嵌入式调试权威索引](../knowledge/EMBEDDED_DEBUG.md)。
