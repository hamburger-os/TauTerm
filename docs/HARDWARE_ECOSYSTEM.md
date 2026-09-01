# TauTerm 硬件生态方向

> 本文描述未来自研工程仪器在 TauTerm 中的软件架构与产品体验方向。它是设计方向，不是硬件规格书，也不构成发布承诺。

## 1. 目标

TauTerm 应成为一系列自研工程分析仪的统一上位机软件。

未来 CAN 分析仪是第一个明显候选。之后只有在能够解决明确工程问题、并且适合统一平台模型时，才继续扩展更多分析仪。

核心思想是：

> **增加一种仪器，应当只是增加一种新的工程数据源，而不是再制造一个新的软件孤岛。**

每一种自研仪器都应直接获得同一套 Workspace、Recorder、Unified Timeline、Signal Lab、Data Lens 与 Automation 能力。

```text
Tau CAN Analyzer ─ CAN/CAN FD ─┐
Serial device ─ UART/RS-485 ───┤
TRDP network ─ Ethernet ────────┼─ TauTerm Workspace
Linux target ─ SSH/journald ────┤     ├─ Recorder / Replay
TCP/UDP service ─ Network ──────┘     ├─ Unified Timeline
                                     ├─ Signal Lab
                                     ├─ Data Lens
                                     └─ Automation
```

## 2. 平台原则

### 2.1 TauTerm 是统一控制与分析环境

在条件允许时，仪器发现、配置、采集、监控、解码、Recording 和 Automation 都应在 TauTerm 内完成。

如果底层固件恢复、驱动修复或生产制造确实需要，可以单独提供小型恢复/配置工具；但日常工程使用应尽量留在 TauTerm 内。

### 2.2 自研硬件获得最深度的集成体验

自研仪器应能够被自动发现、识别，并以尽可能少的配置开始使用。

架构仍可以通过稳定扩展边界支持第三方或通用适配器。自研设备则通过已知能力、硬件时间戳、固件生命周期支持、设备诊断和经过验证的工作流获得最完整的体验。

### 2.3 Capability Negotiation 必须版本化

TauTerm 应通过能力发现了解设备可以做什么，而不是在软件中硬编码某个型号或某个硬件版本的假设。

示例 Capability：

```text
capture.can
capture.can_fd
capture.analog
capture.digital
hardware_timestamp
trigger.hardware
stream.realtime
firmware_update
calibration_info
output.inject
```

新的硬件版本应能够新增 Capability，而不要求修改无关 UI 或其他协议逻辑。

### 2.4 原始采集数据是长期数据资产

采集管线应在足够靠近数据源的位置保留 Raw Frame 或 Sample，使未来 Recording 可以重新 Replay、重新解码或使用新版软件重新分析。

表格和图表只是工程数据的视图，而不能成为唯一持久化形式。

### 2.5 时间是一等工程数据

跨会话、跨仪器分析依赖可信时间戳。

仪器集成模型应考虑：

- Host Receive Timestamp；
- 条件允许时的 Hardware Capture Timestamp；
- 时间戳分辨率；
- 时钟源元数据；
- Device/Host Clock Offset；
- Clock Reset 与 Discontinuity Marker；
- 硬件支持时的多仪器同步能力。

Recorder 应保留足够的时间信息，使 Unified Timeline 能够区分“真实采集时间”和“Host 接收时间”。

### 2.6 仪器工作流必须本地优先

采集、解码、绘图和 Replay 必须能够离线工作。

隔离环境所需的驱动、固件包和校准元数据应提供受控的离线分发方式。

## 3. 面向软件的 Instrument 模型

物理仪器与协议 Session 可以共享会话/事件基础设施，但不应为了统一而强行塞进完全相同的 Adapter 抽象。

未来可以概念性地暴露：

```text
InstrumentManifest
├─ product family
├─ model
├─ device identifier
├─ firmware version
├─ host transport
├─ capabilities
├─ channel definitions
├─ clock information
└─ configuration schema

InstrumentSession
├─ configure()
├─ start_capture()
├─ stop_capture()
├─ send/inject()          # when supported
├─ events / frames / samples
├─ health/status
└─ firmware/update hooks
```

精确 API 应在第一款真实仪器进入实现阶段时再设计。当前最重要的约束是：**仪器数据最终必须进入与协议数据一致的产品级事件管线。**

## 4. 共享数据路径

```text
Instrument Driver
      ↓
Raw Frame / Sample
      ↓
Timestamp + Source Metadata
      ↓
Framing / Decoder
      ↓
Structured Event / Signal
   ├─ Native instrument view
   ├─ Signal Lab
   ├─ Data Lens
   ├─ Recorder / Replay
   ├─ Unified Timeline
   └─ Automation
```

因此，同一份 Raw Capture 可以在不重复采集逻辑的情况下，支撑多个后续视图和分析流程。

## 5. 设备生命周期

仪器平台应把完整设备生命周期视为产品质量的一部分，而不仅仅关注“能不能收数据”。

### 5.1 发现与身份

在硬件支持时，TauTerm 应能够确定：

- 设备系列与型号；
- 稳定设备标识；
- 固件版本；
- 硬件版本；
- Capabilities；
- 通道数量与类型；
- 校准元数据状态。

### 5.2 连接状态

Instrument Session 应暴露具有工程意义的状态，例如：

- unavailable；
- ready；
- configured；
- capturing；
- paused；
- faulted；
- updating firmware。

### 5.3 健康状态与诊断

在硬件支持时，TauTerm 应展示：

- Transport Error；
- Overflow/Drop Counter；
- 设备温度或供电告警；
- 总线/控制器错误；
- 时间戳不连续；
- 固件兼容性问题。

如果这些事件会影响工程分析，它们也应进入 Recording。

## 6. 未来 CAN 分析仪

自研 CAN 分析仪应与 TauTerm 工作流一起设计，而不是先完成硬件、最后再补一个桌面界面。

### 6.1 首版软件侧范围候选

- CAN 2.0A / 2.0B；
- 硬件条件允许时支持 CAN FD；
- 可配置 Nominal/Data Bit Rate；
- Hardware Timestamp；
- 接收与发送/注入工作流；
- Acceptance Filter；
- 总线状态与 Error 可见性；
- Trace Recording；
- 受控的 Trace Replay/Transmit；
- 通过 Data Lens 支持 DBC/Symbol Decode；
- Statistics 与 Bus Load；
- Trigger/Marker 集成；
- 硬件支持时的多通道；
- Firmware Update 与 Device Diagnostics。

除非出现明确产品需求，CAN XL 可以继续作为后续方向，而不是首版要求。

### 6.2 TauTerm 原生 CAN 工作流

CAN Session 不应停留在 Frame Table，而应参与其他 Session 共享的工程上下文。

示例：

- 将 CAN Error Frame 与 Serial Console Message 按时间关联；
- 当某个已解码 CAN Signal 越过阈值时自动添加 Recording Marker；
- 在 Signal Lab 中绘制已解码 CAN Signal；
- 由 CAN Event 触发 SSH/journald 查询；
- 将 CAN Trace 与已记录的设备/网络上下文一起 Replay；
- 比较两次测试的 Capture；
- 针对已解码 CAN 字段执行 Automation。

### 6.3 安全控制

Transmit、Injection 与 Replay 可能直接影响真实系统，因此必须有明确安全边界。

潜在控制包括：

- 清楚区分 Monitor-only 与 Transmit-capable 模式；
- 高影响 Replay/Injection 前显式确认；
- 发送速率与循环次数限制；
- 明确显示当前 Active Transmit 状态；
- 设备或 Host Session 关闭时自动停止；
- 在适用时记录自动发送行为并纳入审计/Recording。

## 7. 多仪器方向

只有同时满足以下条件时，才应考虑新的仪器类别：

1. 存在明确工程问题；
2. 硬件本身具有可信价值；
3. 能自然进入 TauTerm 数据模型；
4. 能与 Recording、Timeline、Signal Lab、Data Lens 或 Automation 形成有意义的交互。

潜在方向可能包括网络/协议分析仪、串口/现场总线接口、混合数字/模拟采集设备，以及面向铁路或工业场景的专用仪器。

平台应避免开发那些数据无法进入共享工作流的一次性仪器。

## 8. 硬件/软件版本兼容

长生命周期工程硬件必须尽早规划兼容性。

推荐具备：

- 版本化 Host-Device Protocol；
- 向后兼容的 Capability Discovery；
- 明确 Firmware Compatibility Range；
- 可恢复的 Firmware Update 路径；
- 量产设备使用签名固件；
- 稳定 Capture/Event Schema；
- Recording 中包含 Migration/Version Metadata；
- 面向工业部署的清晰 Support/LTS 策略。

## 9. 商业关系

自研仪器一旦售出，就应拥有长期可用且有实际价值的本地基础工作流。

理想体验是：

1. 将仪器连接到 TauTerm；
2. 立即获得有实际价值的基础采集与检查能力；
3. 可选使用 Professional/Industrial 能力进行更深入的分析、关联、自动化、报告和支持；
4. 后续增加更多自研仪器时，继续使用同一个工程环境。

即使软件维护期结束，用户也应继续拥有对已购买硬件的基础访问能力。

商业包装原则参见 [COMMERCIALIZATION.md](COMMERCIALIZATION.md)。

## 10. 平台复利价值

硬件生态的长期价值来自共享工作流：

> **一系列不断增长的工程仪器，其数据都能够在同一个本地优先工作台中完成采集、解码、绘图、Recording、Replay，并与远程系统、嵌入式设备和工业网络关联。**

每增加一种仪器，都应强化共享平台；每改进一次共享平台，也应同时提升所有仪器的价值。
