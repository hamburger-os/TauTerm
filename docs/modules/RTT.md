# RTT 调试助手

## 目标

RTT 调试助手为嵌入式目标提供长期运行的 Real Time Transfer 会话，负责调试探针连接、RTT Control Block 定位、多 Up/Down Channel 收发、有限历史、Session Data Log 与 Terminal/Log/HEX 观察工作区。

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
       ┌────────────┼─────────────┐
       ▼            ▼             ▼
 bounded history  LogEngine   presentation batch
                                  │
                              runtime store
                                  │
                         Terminal / Log / HEX
```

`RttRuntime` 通过 `SessionService` 挂到 Container Session，并由 RTT 插件自己的 `SessionRuntimeRegistry<RttRuntime>` 建立弱索引。SessionStore 仍是用户可见连接生命周期的唯一权威所有者；RTT Runtime 只持有插件私有资源与状态。

`EmbeddedDebugManager` 是进程内的物理探针所有权 registry。进入 registry 前，Auto/显式 selector 都先解析为 canonical selector；registry 以 canonical physical probe identity 为槽位，同一探针只有一个活动的 `DebugTargetRuntime`。相同 target/wire/speed 配置复用该 runtime；同一物理探针若请求不同目标配置则显式返回冲突，而不是尝试第二次打开 USB probe。每个 `DebugTargetRuntime` 都由单独 worker 线程唯一拥有 probe-rs `Session`，RTT worker 只通过有界调度队列提交短操作，`Core` 仍只在该 worker 中短生命周期借用。RTT 取得独占的 `rtt` service lease，防止同一物理目标出现第二个 RTT reader；未来变量采样等不同 service 可以复用同一 target worker，而不复制 probe handle。

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

### 已有 J-Link 调试会话

兼容 backend 只连接 `127.0.0.1` 上现有 J-Link RTT TELNET 服务，用于与已经占用 J-Link 的 IDE/Debugger 共存。它不会打开 USB probe，也不会泛化成任意远程 TCP RTT 客户端。

该 backend 按显式 Channel 列表建立 loopback 连接，并遵循 SEGGER RTT TELNET Channel 选择协议。它只报告真实可用能力，不声称支持 probe 枚举、Control Block 定位、完整 Channel metadata 或直接目标控制。

## Channel、发送与自动化

RTT Up 与 Down 是独立方向。同一 index 可以仅 Up、仅 Down，或同时具有 Up/Down。

工作区中的当前观察 Channel 与 SendBar 的发送 Channel 是两个独立状态：

- **观察 / Automation source**：必须有 Up，用于 Terminal/Log/HEX 与 Auto Reply/Lua `on_data`；
- **Send target**：必须有 Down，由公共 SendBar 顶部 `RttSendTarget` 选择；
- Terminal 模式的键盘输入直接写当前 Terminal 对应的 Down Channel，这是终端交互，不是第二套发送栏。

RTT 启用 TauTerm 公共 SendBar。Basic/Command/Auto Reply/Script 继续使用统一 SendBar 产品体验；底层通过协议无关 `AutomationIo` 接入，而不是要求 RTT 根 Session 伪造 `DataPlane`。Lua 的 `send()` 使用当前 Down target，`send_to("rtt:<index>", ...)` 可显式指定 Channel；接收订阅来自当前 Up source。

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

worker 每个 tick 只处理有界数量的控制命令；Down 写入按固定 byte quantum 轮转推进，不能让一个满缓冲 Down Channel 在整个 write timeout 内独占 worker。Native backend 的一次 target-worker Up poll 同样限制 Channel 数与每 Channel read 次数，并以轮转 cursor 推进，避免 RTT 在共享 probe scheduler 上形成一个无界长操作。每次循环仍优先保持持续 Up polling，从而降低日志/Trace 类高吞吐流被发送操作饿死的风险。

## 历史、日志与丢失语义

同一 canonical frame 分别进入三个用途不同的路径：

1. **bounded history**：进程内切回 Session/重建视图时回放；
2. **Session Data Log**：用户显式开启时进入公共 LogEngine，记录为 `[RX][RTT:n]` / `[TX][RTT:n]`；
3. **presentation batch**：短周期批量发送 WebView。

历史缓存具有 per-channel 与 per-session 总预算；超限只淘汰最老历史并累计 history loss。AutomationRx 队列过载只累计 automation loss，presentation queue 过载只累计 presentation loss；两者都不能被描述为原始 RTT 丢失或日志丢失。Session Data Log 自身的队列/磁盘损失继续由 LogEngine 健康状态负责。

未来 SystemView/defmt 等 decoder 必须订阅 canonical RTT frame 或其等价 raw source，不允许创建第二个 RTT reader；目标端 RTT overflow、host acquisition loss、recording loss、presentation loss 也必须保持不同语义。

## 前端运行态与后台生命周期

RTT 的 snapshot、generation、Channel buffer、当前观察 Channel、Send target 与 view mode 由插件级 runtime store 持有，不散落在 `RttSessionView` 的临时 React state 中。切换 Session/Pane 不停止 worker；切回来仍看到同一进程内 runtime 的状态与有限历史。前端缓存同时执行 per-channel 与 per-session 总预算，Log 视图只挂载有界的近期 chunk，避免多 Channel 长时运行把 WebView 内存和 DOM 数量按 Channel 数线性放大。

同一 Saved Session 重连会创建新的 generation；新 generation 到达时清空旧 presentation cache 并重新同步 source/target，禁止旧事件污染新 runtime。

## 生命周期

连接成功边界是 backend 已打开、RTT 已完成 attach/定位并取得初始 Channel metadata；此前公共 Session 不发布 Connected。

正常关闭按“停止接受新工作 → 请求 worker shutdown → flush presentation → backend shutdown → join → runtime registry detach”收敛。运行期 probe 拔出、目标掉电或 backend fatal error 会把 runtime 置为 Faulted，并通过 SessionStore 统一标记 Session 断开；异常断开可使用公共 `retain_terminal` 保留当前进程内只读现场。

## UI

Saved Session 遵循统一两行 presentation：第一行默认名称只在创建时生成，后续配置修改不自动改名；第二行动态显示 probe 简名或 `127.0.0.1:<port>`。

连接页使用统一控件高度/矩阵布局。常用项只展示连接方式、probe、target、wire protocol、固件符号文件和 RTT 定位；speed/core 收入高级设置。工作区复用 Workspace Content surface，不再拥有第二套发送框或额外玻璃 Card。

宽 Pane 使用左侧 Channel rail + viewer；窄 Pane 使用 `session-pane` container query 把 Channel 列表调整为顶部横向区域。viewer 只包含 Terminal / Log / HEX。

## 扩展边界

SuperWatch/实时变量和 SystemView/RTOS Trace 会共享“probe/target 所有权、固件符号、调度、原始观测事件与丢失语义”，但它们不是 RTT 协议本身。native probe 所有权已经提升到共享 Embedded Debug runtime：RTT 只是其中一个 service，不能再创建插件私有 probe worker。

未来直接内存采样必须通过同一个 `EmbeddedDebugManager` 获取 target worker，并使用独立 service lease/公平的短操作调度；SystemView/defmt 则订阅 RTT canonical raw source，不允许建立第二个 RTT reader。变量/DWARF/SVD 解析、采样策略和 Trace decoder 都留在各自 observation domain，不能塞进 RTT backend 或公共 Kernel。

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
