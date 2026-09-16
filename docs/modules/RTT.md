# RTT 调试助手

## 目标

RTT 调试助手为嵌入式目标提供长期运行的 Real Time Transfer 会话。它负责调试探针连接、RTT Control Block 定位、多 Up/Down Channel 收发、有限历史缓存和 Terminal/Text/HEX 工作区，并保持 TauTerm 既有 Session/Workspace 生命周期语义。

RTT 是多通道目标内存通信机制，不等价于单个串口字节流。本模块因此使用 **Container Session + 插件私有 Runtime**，而不是把任意一个 RTT Channel 伪装成根 Session `DataPlane`。公共 Kernel 不解释 RTT、调试探针、Control Block 或 Channel 元数据。

## 当前方案

```text
Saved RTT Session
        │
        ▼
SessionStore / Container Session
        │
        ▼
     RttRuntime
        │
        ▼
 single-owner RttWorker
   ┌───────────────┴───────────────┐
   │                               │
Native debug-probe backend   Existing J-Link backend
   │                               │
probe-rs Session / RTT       127.0.0.1 RTT TELNET
   │                               │
   └──────────── Raw RTT Channels ─┘
                    │
             bounded history
                    │
          batched presentation
                    │
              Custom Workspace
          Terminal / Text / HEX
```

`RttRuntime` 通过 `SessionService` 挂到 Container Session，并由 RTT 插件自己的 `SessionRuntimeRegistry<RttRuntime>` 按稳定 Session ID 建立弱索引。SessionStore 仍然是用户可见连接生命周期的唯一权威所有者；RTT Runtime 只拥有插件私有资源和状态。

调试探针对象只由单独的 `RttWorker` 线程拥有。Tauri command 不直接锁定或借用 probe/session/core，而是通过有界命令队列向 worker 请求写入、刷新 Channel 或关闭。probe-rs `Core` 只作为一次读写操作的短生命周期借用，不跨线程、也不保存在共享状态中。

## Backend

### 原生调试探针

原生 backend 使用固定版本的 `probe-rs` Rust library，负责探针发现、SWD/JTAG 选择、目标 attach、CPU Core 选择、RTT Control Block 定位以及 Up/Down Channel 读写。当前配置支持：

- 自动选择唯一探针，或显式选择 probe selector；
- 目标芯片；
- SWD / JTAG；
- 自动或指定接口速度；
- CPU Core index；
- 自动扫描目标 RAM、精确 Control Block 地址或显式搜索范围；
- 有界 attach、poll 和 write timeout。

如果同时发现多个探针，自动模式失败关闭，要求用户明确选择；不得猜测目标设备。

Channel 刷新不是重新读取首次 attach 时缓存的向量，而是使用当前 Session/Core 与原定位策略重新 attach RTT Control Block，成功后一次性替换 RTT handle 与 Channel metadata。这样目标固件在运行期重新初始化 RTT 后，刷新动作才具有真实语义；失败时继续保留原 runtime 状态并返回结构化错误。

### 已有 J-Link 调试会话

兼容 backend 只连接 `127.0.0.1` 上现有 J-Link RTT TELNET 服务，用于 TauTerm 与已经占用 J-Link 的 IDE/Debugger 共存。它不会打开 USB 探针，也不会把该入口扩展成任意远程 TCP 客户端。

该 backend 按配置的 RTT Channel 建立独立 loopback 连接，并按 SEGGER RTT TELNET Config String 契约在连接建立后立即选择对应 Channel。每次 worker poll 对单个连接的读取工作量有界，避免持续高吞吐 Channel 长时间占用 worker、饿死其它 Channel 或控制命令。

它显式报告能力降级：不声称能够发现所有 Channel、读取完整 Channel metadata、定位 Control Block 或控制目标 Core。前端根据 runtime capability 展示真实能力；例如 Existing Session 不显示原生 Channel 重新发现操作，公共 UI 也不按 backend ID 伪造功能。

## Channel 模型

RTT Up 与 Down 是独立方向。Rust domain model 对每个逻辑 index 分别保存可选 Up/Down 元数据；UI 可以把相同 index 合并成一行显示 `↑` / `↓`，但不能把它们错误建模成天然双向的单 stream。

一个 Channel 可以是：

- 仅 Up；
- 仅 Down；
- 同 index 同时具有 Up 与 Down。

Channel 0 默认使用 Terminal 视图，其他 Channel 默认使用 Text；用户可在 Terminal / Text / HEX 之间切换。不同 Channel 的 Terminal 实例彼此独立，切换 Channel 不能把两个 Channel 的屏幕历史混在同一个 xterm 状态中。首版不自动猜测 binary/text，也不在 RTT Core 中解析 defmt、ELF/DWARF 或其它上层格式。

全局 SendBar 对 RTT 禁用。发送目标属于具体 Down Channel，由 RTT 工作区自己的输入边界处理；公共 `SessionIo` 不增加 RTT 私有 target 语义。

## 数据与后台生命周期

RTT worker 与 React 组件生命周期分离。切换 Session、切换 Pane、清空 Pane 或把会话移动到其它 Pane 都不会停止 RTT polling；Workspace 只负责当前进程内 custom view 的展示连续性，真实后台资源始终由 `RttRuntime` 持有。

接收路径：

```text
RTT Up Channel
    ↓
RttWorker
    ↓
短窗口/大小阈值合并
    ├─ bounded per-channel/session history
    └─ rtt-event presentation stream
              ↓
          WebView view state
```

Rust 历史缓存同时具有 per-channel 和 per-session 总预算。超限时只淘汰最旧历史，并累计 dropped chunk/byte；UI 必须可观察历史缺口。这个缓存只解决进程内视图重建和短期回放，不是长期 Recorder/Evidence Path，也不能无限增长。视图首次恢复时优先读取当前缓存中最近的一段数据，而不是从最旧片段开始，保证重新进入后台 Session 时首先看到当前现场。

每个展示 chunk 带单调 sequence、时间戳、Channel index 和原始 payload。未来 Recording/Replay 接入时应在靠近原始 RTT 数据源的位置进入共享工程事件管线，而不是依赖当前 WebView 文本结果反向恢复原始数据。

同一 Saved Session 重连会创建新的 RTT runtime generation；前端以新的 `connected_at` 为代际边界清空旧 generation 的 presentation cache，避免新 runtime 从头计数的 sequence 与旧历史互相去重或混合。

## 生命周期

连接成功的正式边界是 backend 已打开、RTT 已完成 attach/定位并取得初始 Channel metadata；此前公共 Session 不能发布 Connected。RTT runtime 只保留真实可观察的 `Idle → OpeningBackend → Running/Faulted → Stopping` 阶段，不为 backend 内部的瞬时步骤建立无法稳定观察的伪状态。

```text
Disconnected
   ↓ connect
Container Session registered
   ↓
RttWorker starts
   ↓
open backend / attach RTT
   ↓
Running
   ↓
session-connected
```

正常断开：停止接受新命令 → 通知 worker shutdown → flush 待展示数据 → backend shutdown → worker join → runtime registry detach。

运行期探针拔出、目标掉电、RTT transport/loopback 关闭或 backend fatal error 会把插件 runtime 置为 Faulted，并通过 SessionStore 统一标记 Session 断开。异常断开使用公共 `retain_terminal` 语义保留当前进程内最后 snapshot 与 bounded history，用户切走再回来仍可只读查看故障现场；用户主动断开或从磁盘加载的普通离线 Saved Session 不保留一个不存在的 runtime。插件错误码保留在 RTT snapshot；公共 `session-disconnected` 仍使用公共断开语义，不能把 RTT 私有错误枚举泄漏成新的 Kernel 状态。

## 错误边界

RTT 内部错误使用稳定 code + message，至少区分探针缺失/歧义/占用/权限、目标 attach/core、RTT Control Block、Channel、J-Link 服务、写超时、目标或探针断开以及取消。前端本地化和展示不得通过解析第三方库原始字符串决定业务语义。

第三方 probe/backend 错误只在插件边界映射。Kernel、`StatusBarContext`、Workspace 与通用 Session presentation contract 不增加 RTT 私有字段。

## UI

RTT custom view 复用 Workspace 已有 Content surface；插件内部只使用平面区域、divider、Channel rail、viewer 和 input bar，不再套第二层基础 Liquid Glass Content/Card 壳体。

宽 Pane 使用左侧 Channel rail + viewer；窄 Pane 通过 `session-pane` container query 把 Channel 列表折叠为顶部横向选择区。Terminal/Text/HEX 和 Down 输入始终保持可达。

## 当前明确不做

首版不实现 SWO/ITM、Flash、Memory/Register、SVD、源码级 Debugger、defmt 解码或通用 Debug Probe Framework。只有当第二种真实探针能力进入产品、并与 RTT 出现稳定共享所有权需求时，才从现有实现中提取通用 Debug Probe runtime；不为未来假设提前把 Kernel 泛化。

## 代码锚点

- `src/plugin-manifests/rtt.json`
- `src-tauri/src/plugins/rtt/`
- `src/plugins/rtt/`
- `src-tauri/src/plugins/catalog.rs`
- `src/plugins/catalog.ts`

## 何时更新本文

修改 RTT backend、探针所有权、Control Block 定位、Channel 模型、历史/丢失语义、后台生命周期、工作区数据视图或将 RTT 能力抽到共享 Debug Probe 层时，必须同步更新本文。

外部语义与上游接口依据见 [嵌入式调试权威索引](../knowledge/EMBEDDED_DEBUG.md)。
