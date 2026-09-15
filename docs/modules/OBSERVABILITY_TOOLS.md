# 数据、日志与工程工具设计

## 目标

这一模块覆盖两类共享能力：

1. **可观测性数据路径**：高频 Session 数据如何送往前端、如何记录日志、如何形成统计；
2. **无连接工程工具**：校验、编码、位操作、数值转换和轻量协议解析等不应绑定某个协议 Session 的辅助能力。

它们都属于跨协议公共能力，但不能因此进入协议核心状态机。

## 当前方案

### 数据批处理

高频接收数据先在 Rust 侧按短时间窗口和大小阈值合并，再以 Base64 事件发送前端，降低大量小包造成的 IPC/JSON/渲染开销。关闭时必须 flush 已缓存数据。

DataBatcher 属于 **Presentation Path**：极端过载时允许丢弃显示数据块以保护 UI，但每次丢弃都会累计计数，并按 1/2/4/8… 次节流发送 `session-display-overflow`；同一节流规则也用于后端 overflow warning，避免在过载时用日志本身制造新的队列压力。这个降级语义不能复制到未来 Recorder/Evidence Path。

### 日志

日志分成两个语义不同的流：

- **System Log**：应用诊断事件，受启用状态和最低级别控制；
- **Session Data Log**：用户对某个 Session 显式启动后记录的 TX/RX transcript，属于 best-effort 调试日志，不是证据记录器。

LogEngine 使用有界生产者/消费者队列和独立写线程完成磁盘 I/O。共享入口不是无差别竞争：Session 数据和 System 事件分别拥有软预算，必须给控制命令保留队列容量；Session 数据达到自己的预算后先丢弃并累计 loss counter，不能把 Start/Stop/Clear 等控制命令一起挤出队列。控制命令仍保持 fail-fast ACK 语义，调用方不能为了等待日志队列而阻塞通信热路径或 Tauri 命令线程。

Session Data Log 有两级开关。`session_enabled` 只表示应用允许使用 Session 日志；真正的数据生产还要求对应 Session 已成功创建活动 writer。未显式开始记录的 Session 必须在生产端短路，不能把 TX/RX 数据塞入共享日志队列后再由消费者丢弃。能够在调用点判断活动状态的发送路径还应在构造/复制日志 payload 前短路，避免日志关闭时仍承担无意义的数据复制成本。

`system_enabled/system_level` 只控制应用诊断日志。Rust `log` facade 的全局 ceiling 保持可接收 Trace，真正的运行时级别过滤由 LogBridge 统一负责，因此 Settings 中的 Debug/Trace 不能在到达 LogBridge 前被静态 `Info` 上限提前吞掉。前端 `log_event` 与 Rust `log::*` 进入同一最低级别语义；非法级别不得伪装成普通日志事件。

日志目录属于**进程启动期一次性资源**，不是运行时可变设置。LogEngine 创建时只建立有界队列和 LogBridge，不启动消费者、也不创建文件；Tauri setup 解析出最终可写的绝对目录后一次性完成目录配置并启动消费者。配置完成前产生的启动事件保留在有界队列中，随后统一写入最终目录。目录绑定成功后本进程不允许再次切换，因此 `get_log_dir`、设置 UI 和实际 writer 始终指向同一个位置，也不会因为 `tauri dev` 的当前工作目录而隐式创建 `src-tauri/logs`。

所有可从 IPC/ConfigStore 到达的日志参数都必须由 Rust 再验证。单文件大小、buffer、flush interval、retention 等不能只依赖前端 slider 范围；非法值必须在发布运行态之前失败，不能出现 `flush_interval=0` 忙轮询、零大小无限分卷等状态。配置仍采用 ConfigStore snapshot 先持久化、后应用运行态；若 LogEngine 运行态应用失败，必须恢复旧 ConfigStore snapshot 并把失败显式返回给 UI。

#### Flush 与分段存储

`flush_interval` 表示最大缓冲驻留周期，而不是“队列连续空闲这么久才 flush”。消费者维护绝对 flush deadline；即使 Session 持续有数据进入，deadline 到达后也必须刷新全部活动 writer。这样低速但持续的流量不会因为永远触发不到 `recv_timeout` 的 idle timeout 而长期留在用户态 buffer。

System Log 与 Session Data Log 共用同一组文件大小、buffer、flush 和 retention 存储策略，但保留不同格式语义。两者都使用**只追加分段**：达到阈值时关闭当前 segment 并新建下一个，禁止原地读取尾部、truncate、重写旧文件。Session 每个 segment 都写独立 Header，至少能识别 Session、Endpoint、开始时间、数据模式、segment 编号和 TauTerm 版本；单独拿到任意一个 rotated segment 也必须可以理解其来源。Session 数据行使用带时区的完整时间戳，不能只记录跨午夜后会失去日期语义的 `HH:mm:ss`。

Session 文件名的唯一性来自不可变 Session 身份、进程/启动实例信息和 segment 序号，用户可修改的 Session 名称只进入 Header，不再承担文件唯一标识，也不能把路径字符带入文件系统命名。System Log 同样采用独立 segment，避免单个日期文件无限增长。

#### Retention、容量与清理

Retention 不是只在应用启动时运行一次。消费者启动时先执行一次维护，随后在运行期间周期执行，因此应用连续运行数天时过期文件仍会被回收。目录同时有独立于 `retention_days` 的总容量保护上限；达到上限时只从**已经关闭且不属于当前活动 writer** 的 segment 中按时间淘汰，绝不能 truncate 活动文件。

“清除所有日志”继续由唯一持有 writer 的消费者线程执行 `close/flush → delete → reopen → ACK`。活动 Session reopen 后必须创建新的 segment 并重新写 Header，不能在 Linux 上继续向被 unlink 的 inode 写入，也不能在 Windows 上依赖删除仍打开的文件。

#### 失败与健康状态

日志仍然是 best-effort：通信成功不能因为日志磁盘慢、队列满或磁盘不可写而失败。所有丢失必须可观察。健康状态除 System/Session 总丢失计数外，还区分队列丢弃、文件写入失败和存储维护失败，并暴露当前 queue pressure、活动 Session writer 数与目录保护上限。System sink 打开/写入失败后采用有限退避，不能对每个后续事件立即重复打开失败路径形成 I/O/诊断风暴。

System Log 文件自身无法打开/写入时，消费者不得再调用同一个 `log` bridge 递归记录该失败；只使用 stderr 诊断并累计 loss counter。System Log 持久化前还必须经过最终防御性 sanitizer：至少覆盖常见 password/passphrase/token/API key/authorization/cookie/private-key 形式，并把 CR/LF 转义为单条物理记录，避免任意前端/远端错误字符串伪造额外日志行。真正的凭据仍要求调用方在格式化日志前就移除，sanitizer 不能成为传输 Secret 的理由。

Session Data Log 与 System Log 的安全边界不同：Session Data Log 的用途就是在用户明确启动后记录真实线路 TX/RX，所以不能为了“脱敏”任意改变 payload。它必须依靠显式启动、独立生命周期、文件边界和未来 Recorder/Evidence Path 的更强合同来保护。

启动 Session Log 与清除日志的 ACK 等待必须运行在 blocking worker，而不是占用同步 Tauri 命令分发路径；日志控制命令使用有界队列的 fail-fast 入队语义，队列过载时向 UI 明确返回错误。

### 诊断与性能合同

前端未捕获异常通过受限诊断桥进入 System Log。Settings/About 可以导出版本化、脱敏的诊断 JSON，包含构建/平台信息、ConfigStore/凭据后端健康、日志丢失计数、插件元数据与按插件聚合的 Session 状态；凭据、端点、Session 名称、raw payload、System Log 内容与 Session Data Log 不进入诊断包。保存路径由 Rust 发起的 native dialog 决定，WebView 不接收任意目标路径。

性能与长稳验证是独立工程合同：release-mode performance workflow 记录固定 32 MiB I/O dispatch 与原子持久化的趋势数据；reliability workflow 反复创建/发送/Shutdown I/O 生命周期并输出 JSON。Hosted Runner 的性能数值在形成稳定历史前只作为趋势证据，不凭单次波动设置拍脑袋阈值。

### 统计与工程工具

Stats renderer/状态区消费 Session 统计信息。右侧工程能力分成两个明确层级：

1. **Protocol Inspector / 协议检查器**：对用户显式提供的一段文本或字节做离线识别、字段解释、校验与诊断；
2. **Quick Tools / 快捷工具**：对字节、数值、CRC、编码、位、C 结构布局与常用工程参数做本地纯计算。

两者都属于无连接能力，不创建 socket、串口或远端 Session，也不自动订阅 Session I/O。

#### Protocol Inspector

协议检查器使用 `src/protocols/` 下的注册式模型，而不是在 UI 组件中按协议堆叠条件分支。所有 Inspector 输出统一包含：

- 带 `byte` / `char` 单位的字段 range，可与原始数据联动高亮；
- 独立的结构、checksum、协议语义 check；
- 带 severity 与可选 range 的 issue；
- 自动识别结果与置信度；
- 对有请求/响应语义的协议保留显式 direction。

当前 Inspector 覆盖：

- Modbus RTU：CRC-16/MODBUS、常用 01/02/03/04/05/06/0F/10 请求/响应与异常响应；
- Modbus ASCII：ASCII HEX/LRC 与同一套 Modbus PDU 解释；
- Modbus TCP：MBAP + PDU，不错误附加串行 CRC/LRC 语义；
- AT 文本：命令回显、信息响应/URC、prompt、最终结果与通用参数拆分；
- NMEA 0183：语句/talker/type/字段与 XOR checksum，常用 GGA/RMC 字段提供通用解释；
- Raw Frame：只展示规范化字节，不暗示存在协议语义；
- Custom Schema：用本地 JSON schema 描述固定 offset 的整数、浮点、bytes、ASCII/UTF-8、reserved、bitfield、enum/expected 约束、固定/remaining/`lengthFrom` 长度与可选 CRC 字段。动态长度只能引用此前已经解析出的非负整数字段，非法引用或越界必须失败关闭。

自动识别只在高置信模式下识别有明显 framing/checksum 特征的协议；无法可靠判断的 HEX 退化为 Raw Frame，普通文本退化为低置信 AT/Text 分析，不能伪造确定协议结论。

**未来 Modbus Session 与这里不是两套产品能力。** Inspector 是离线 decoder/validator；未来在线 Modbus Session 负责 Serial/TCP transport、request lifecycle、polling、timeout/retry、设备数据视图。功能码、PDU/ADU、异常码和 validation 规则必须复用同一个 canonical Modbus protocol core，禁止分别在 Session 与 Inspector 中维护两份功能码表和解析逻辑。

#### Quick Tools

工程工具使用统一的严格 ByteInput parser；支持连续 HEX、分隔 HEX、`0x` token、`\xHH`、冒号/横线和 C 数组形式。任何非法 token、奇数 nibble 或不完整 endian group 都失败关闭，不能部分解析后返回看似有效的结果。纯函数错误统一返回 typed `ToolResult`，UI 再完成本地化，不再用 `[Error: ...]` 字符串作为业务协议。

当前快捷工具包含：

- **Data Inspector**：同一份 bytes 同时解释为 HEX、ASCII/control、严格 UTF-8、8/16/32/64 位 signed/unsigned、BE/LE、Float32/64 与合法 Packed BCD；
- **CRC Lab**：SUM8/16/32、XOR、规范化 CRC-8/16/32 presets、公开参数/check vector、自定义 Poly/Init/RefIn/RefOut/XorOut、尾部 CRC 验证与 BE/LE 追加；
- **Encoding**：UTF-8/HEX、严格 Base64 与可选忽略空白兼容模式、URL、任意精度整数进制、Float32/64、16/32/64-bit endian、Packed BCD；
- **Bits / C Layout**：8/16/32/64-bit BigInt 位运算、signed/unsigned 结果、bit toggle/range mask，以及 ILP32/LP64/LLP64 + packing 的 C 结构布局估算；
- **Engineering**：串口 frame timing、Unix/HEX timestamp、IPv4/CIDR 与 Byte Diff。

C 结构布局不是编译器 ABI 认证器；位域、嵌套 struct/union 和未知类型失败关闭。文本转 bytes 的默认工程语义是 UTF-8；HEX → UTF-8 默认 strict/fatal，非法序列不能静默变成 replacement character。

Quick Tools 的各工具面板保持 mounted，因此在同一个 Session 的右侧栏中切换类别不会丢失当前输入。一级类别在窄右侧栏中使用统一主题 Select，而不是横向滚动 Tab；工具内部少量、高频互斥选项仍可使用 selector strip。协议检查器、Data Inspector、Encoding 和 Checksum 记录最近 10 条有效输入并允许运行态固定；历史只存在于当前前端组件生命周期，不写入磁盘，也不进入 Session Library/Workspace 持久化资产模型。

Session 与 Inspector 之间只允许**用户显式数据桥**：xterm 选区可通过右键“在协议检查器中查看所选内容”发送；Serial/TCP Dual、TCP Text/Hex 单栏以及 UDP datagram 网格都可对当前行/报文执行“在协议检查器中查看此帧”，且始终传递该行保存的原始 HEX，而不是从显示文本反推字节。事件携带 `sessionId` 并只由对应 Session 的 Inspector 消费，同时显式展开右侧栏。Inspector 不自动监听后台 RX/TX、不复制所有 Session 流量，也不因此承担 Recorder/Evidence Path 职责。

CRC canonical `123456789` check vector、严格 byte parsing、64-bit conversion/bitops、ABI layout、Modbus RTU/ASCII/TCP、AT、NMEA、Custom Schema、Data Inspector 与工程换算由 `npm run check:engineering-tools` 的合同测试覆盖，并作为 CI gate。

## 数据流

```mermaid
flowchart TB
  IO["Session I/O"] --> Batch["Presentation: DataBatcher"]
  Batch --> UI["Terminal / Renderer / Stats"]
  Batch --> Loss["显式 overflow"]
  IO --> Gate["Active Session Log Gate"]
  Gate --> Log["Best-effort LogEngine"]
  Log --> Files["Append-only Segments"]
  Log --> LogLoss["显式分类 loss counters"]
  IO -. future .-> Evidence["Recorder / Evidence Path"]
  Tools["工程工具"] --> Local["本地纯计算/轻量解析"]
```

## 设计边界

- 高频数据批处理不能改变字节顺序和 Session 归属。
- 丢包、队列满、磁盘失败或截断必须可观察，不能静默制造“完整数据”的假象。
- 未启动 Session Log 的会话不能占用日志队列；能够提前判断的通信热路径还应避免构造日志 payload。
- 活动日志文件只允许 append/flush/close/rotate，不允许 retention 或容量维护原地改写。
- System Log 必须经过统一 sanitizer；Session Data Log 则保持真实线路字节，不把两种安全语义混为一谈。
- 工程工具默认是本地纯函数式能力，不应暗中建立网络连接。
- Modbus/AT 等 parser 只解析它明确支持的范围；协议标准依据记录在 `docs/knowledge/`，不能把工具 UI 当成标准。
- 性能参数只有在成为长期合同后才写入本文；具体实现常量仍留在代码。Benchmark 输出是回归趋势证据，不是产品宣传跑分。
- Diagnostic Bundle 只能包含经过明确白名单选择和脱敏的信息，不能变成绕过日志/凭据边界的数据导出通道。

## 代码锚点

- `src-tauri/src/kernel/data_batcher.rs`
- `src-tauri/src/kernel/log_engine.rs`
- `src-tauri/src/kernel/log_writer.rs`
- `src-tauri/src/security/log_sanitizer.rs`
- `src-tauri/src/ipc_transport.rs`
- `src/components/Tools/`
- `src/renderers/StatsDashboardRenderer.tsx`
- `src/components/Settings/panels/LoggingSettings.tsx`

## 何时更新本文

修改数据批处理语义、日志数据模型/信任边界、日志存储与丢失语义、统计所有权、工程工具范围或轻量协议 parser 的定位时，必须同步更新本文。
