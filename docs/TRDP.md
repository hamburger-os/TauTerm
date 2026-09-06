# TRDP 会话指南

本文描述 TauTerm 当前 `master` 分支的 TRDP Node / Monitor 工作流、实现边界与已知限制。它是用户和贡献者理解 TRDP 子系统的长期工程文档；协议安全认证、未来能力与未实现设计不在本文中冒充现有功能。

> **安全边界：** TauTerm 的 TRDP 功能用于诊断、开发与互操作测试。当前实现不执行 SDTv2 / SDTv4 安全语义验证，也不声明任何安全认证或安全完整性保证。涉及安全相关系统时，应使用适用标准、经批准的工具链和独立验证流程。

## 1. 会话模型

TRDP 使用 TauTerm 的 Headless custom session。每个 TRDP Session 在应用层仍由统一 SessionStore 管理，但协议数据面不经过全局 SendBar。

| 模式 | 用途 | 运行时 |
|---|---|---|
| **Node** | 主动参与 TRDP 通信 | 启动独立 `tauterm-trdp-bridge`，由 vendored TCNOpen 3.0.0.0 承担 PD/MD wire semantics |
| **Monitor** | 被动抓包与离线分析 | 离线 pcap/pcapng 由 Rust 解析；实时抓包按需启动 sidecar 并动态加载系统 pcap 兼容运行库 |

Node 与 Monitor 是不同职责的会话模式。Node 面向“参与通信”，Monitor 面向“观察通信”；两者共享报文检查、XML/Dataset 等诊断概念，但不应把 Monitor 变成隐式发送端。

## 2. Node：PD / MD 对象

当前 Node 支持以下对象：

| 对象 | 生命周期 | 当前语义 |
|---|---|---|
| PD Publisher | 持续 | Start 后周期发布，Stop 释放 publisher |
| PD Subscriber | 持续 | Start 后订阅并持续接收，支持 timeout policy |
| PD Request | 一次发送 + 回复窗口 | UI 使用 Send 语义；native 侧会在回复窗口内保留 subscriber handle |
| MD Notify | 一次发送 | 发送 Mn |
| MD Request | 一次发送 | 支持 UDP/TCP，请求后接收 Mp/Mq/Me 等会话事件 |
| MD Listener/Replier | 持续 | 监听 Mr；可配置 Reply (Mp) 或 ReplyQuery (Mq)，ReplyQuery 可等待 Confirm (Mc) |

持久对象的状态应以 native ACK 为准，而不是以“命令已写入 stdin”为成功依据。一次性对象在 UI 中表现为动作，不应伪装成长期 Running 对象。

### A/B Link 与 redundancy

Link A / Link B 表示 TauTerm Node 的两个独立 TRDP Application Session / 网络路径。它们与 `redId` 的 redundancy 语义是两个不同维度：

- `link = a | b | both` 决定对象使用哪些本地 Link。
- `redId` 是 TRDP redundancy group 标识。
- Leader / Follower 状态属于指定 Application Session 上的 redundancy group，而不是“Link A 固定 Leader、Link B 固定 Follower”。

因此，不应从 A/B 自动推导 redundancy 状态。

## 3. XML 与 Dataset

连接配置中的 XML 路径是可选的预设路径。当前会话进入后仍需要执行“导入 TRDP XML”才能把文件载入 Dataset/Telegram 工具。

导入后 TauTerm 可以：

- 建立 ComID → Dataset 映射；
- 预览 Dataset 字段与 Telegram 元数据；
- 用结构化字段编码/解码 Payload，同时保留 Raw HEX 作为 wire truth；
- 从明确标记为 PD 的 Telegram 生成 PD Subscriber 模板；
- 读取 PD/MD 端口，并识别 SDT 相关元数据。

当前 XML importer 是 TauTerm 自己实现的**有限子集解析器**，不是完整 XSD validator，也不代表对任意 IEC 61375 / TCNOpen XML 配置都能无损理解。导入警告应被视为需要人工检查的诊断结果。

## 4. Monitor 与抓包

### 实时抓包

实时 Monitor 支持一个或两个抓包接口。Auto filter 根据会话的 PD UDP、MD UDP 与 MD TCP 端口生成 BPF；切换到 Custom 后，过滤表达式按用户输入使用。

平台运行时要求：

| 平台 | 实时抓包 |
|---|---|
| Windows | 需要用户安装提供 pcap API 的抓包运行库；TauTerm 不随包分发 |
| Linux / macOS | 动态加载系统 libpcap |
| 所有平台 | 离线 pcap/pcapng 不依赖实时抓包运行库 |

实时路径会保留 Link A/B 来源，并对 MD/TCP 做流重组。内存中的实时抓包当前最多保留 50,000 帧；超过上限后淘汰较早帧并累计 dropped count。

### 离线抓包

当前 Rust 离线解析器支持 classic pcap 和 pcapng 的主要输入路径，并识别 Ethernet、RAW IP、Linux cooked capture 与 NULL/loopback 等已实现 link type。它会校验 TRDP header CRC、协议版本范围与声明 Payload 是否完整。

当前限制：

- 离线 MD/TCP 尚未像实时路径一样做 TCP stream reassembly，因此跨 TCP segment 的完整 MD 报文可能无法被解析。
- 离线文件当前一次性读入内存，并把解析结果整体返回前端；超大抓包文件不是当前优化目标。
- pcapng 解析聚焦当前实现需要的 Section Header / Interface Description / Enhanced Packet 等路径，不应视为通用 pcapng 编辑器。

保存 pcapng 时，TauTerm 使用当前 capture buffer，而不是把历史滚动事件日志混入导出；可用时保留原始 frame、link type 与 A/B interface provenance。

## 5. Workspace

仓库中的 `samples/trdp/tauterm-lab-profile.json` 展示了实验性 Workspace 结构。

**当前实际导入行为只应用 Workspace 的 `name` 与 `objects`。** 示例中的 `links`、`xml`、`monitor` 与 `auto_start` 等字段目前不会作为完整 Session 配置自动应用，也不会因为导入而自动开始发送。

因此，当前版本应把“Import Workspace”理解为对象模板导入，而不是完整 TRDP 工程恢复。研发阶段后续若重做 Workspace schema，应优先一次性收敛为有版本、强类型、可验证的单一模型，而不是继续扩展当前松散 JSON 行为。

## 6. 运行时与进程边界

Node 使用独立 sidecar 隔离 TCNOpen C runtime、VOS thread/socket lifecycle 与 Tauri 主进程。TauTerm Rust 层负责：

- Session 生命周期与 sidecar 启停；
- JSON-lines IPC 与前端事件转发；
- XML / Dataset；
- 离线 pcap/pcapng；
- Tauri command 边界。

sidecar 负责：

- TCNOpen Node Application Session；
- PD/MD 对象与回调；
- 实时抓包与实时 MD/TCP 重组。

Release 构建只从受控 resource / executable 位置解析 helper；仓库/CWD 相对查找仅用于 debug build。开发环境可以显式设置 `TAUTERM_TRDP_BRIDGE` 覆盖 helper 路径。

## 7. 当前实现限制与贡献约束

下列行为是当前实现边界，不应在 README、UI 或发布说明中描述成已经解决：

| 领域 | 当前限制 |
|---|---|
| Session handshake | sidecar 的启动命令与 native open 结果尚未形成严格的 request/response 握手状态机 |
| IPC | native error event 目前没有统一 request id，多个并发操作缺少严格的结果关联 |
| Capture | live 与 offline 使用两套 decoder；offline MD/TCP 无 stream reassembly |
| 大文件 | pcap/pcapng 与前端 packet list 仍以整批内存模型为主 |
| XML | 当前 importer 为有限子集解析，不是完整 XML/XSD 语义验证 |
| Workspace | 当前不是完整 Session/工程状态恢复模型 |
| Safety | 不执行 SDTv2/SDTv4 safety validation，不声明 safety certification |

在这些边界被重构前，新增 TRDP 功能应避免再创建第三套报文模型、第三套配置格式或新的隐式状态源。

## 8. 构建、样例与测试

TRDP native helper 与 reference peer 的构建说明见 [BUILDING.md](BUILDING.md)。整体进程边界见 [ARCHITECTURE.md](ARCHITECTURE.md)。

仓库提供：

- `samples/trdp/*.xml`：PD/MD/Dataset 示例；
- `samples/trdp/tauterm-lab-profile.json`：实验性 Workspace 示例；
- `tools/trdp-test-peer/`：小型 TCNOpen reference peer；
- `.github/workflows/trdp-native.yml`：native 构建、抓包接口枚举与 Linux PD/MD interoperability 覆盖。

协议改动至少应验证 PD 发布/订阅/请求、MD UDP、MD TCP、ReplyQuery → Confirm、A/B Both、XML/Dataset 与 capture parser 的相关路径，并保持“诊断工具，不是 safety validator”的边界不变。
