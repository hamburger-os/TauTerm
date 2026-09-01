# TauTerm 产品战略

> 本文记录 TauTerm 的长期产品方向。它比具体版本路线更长期、更稳定，只有当产品决策本身发生变化时才应修改。

## 1. 产品愿景

**TauTerm 是面向连接系统的开源、本地优先工程工作台。**

它应当让工程师能够在同一个工程上下文中连接、观察、理解、自动化并复现由远程计算机、嵌入式设备、网络协议和物理工程仪器共同组成的系统。

公开产品口号保持为：

> **TauTerm —— 一台终端，服务机房与实验台。**

更完整的产品类别定义是：

> **面向连接系统的开源工程工作台。**

## 2. 核心用户

TauTerm 面向三类彼此关联、但在产品战略中承担不同角色的用户。

1. **嵌入式开发者是产品根基。** 设备 Bring-up、串口通信、二进制数据、实时信号以及与硬件紧密相关的工程流程必须始终是一等能力。
2. **连接系统与设备研发工程师是产品中心。** 这类用户会在同一个工程任务中同时处理设备、Linux 服务、网络协议、日志、测试工具和自动化。
3. **工业与铁路工程团队是长期主要商业客户。** 他们更重视离线运行、长时间稳定性、可追溯性、专业协议、可重复流程、受控部署和长期支持。

当网络与基础设施工作自然属于连接系统工程的一部分时，TauTerm 也可以很好地服务网络工程师和基础设施工程师。

## 3. 产品原则

### 3.1 本地优先

核心工程能力必须能够在没有账号、云服务或互联网连接的情况下工作。

SSH、Serial、网络调试、Recording/Replay、Signal Lab、Data Lens、协议分析和自动化都应当能够在实验室、工厂、铁路环境和隔离网络中正常使用。

未来可以提供同步、授权、协作或分发等可选在线服务，但这些服务不能成为工程工作台运行时的必要依赖。

### 3.2 完整开源 Core，商业化专业价值

开源 Community/Core 应当始终是一款完整、可信、真正有用的工程工具。基础 SSH/SFTP、Serial、TCP/UDP、本地 Shell、协议调试、脚本和扩展能力不应为了推动付费而被人为削弱。

商业产品应围绕高价值专业工作流、官方高级模块、团队协作、企业治理、支持服务和行业能力收费。

Community/Core 仓库可以继续采用 MIT OR Apache-2.0；可选商业模块可以通过清晰的包、仓库或插件边界与开源 Core 隔离，并采用独立商业许可证。

### 3.3 工程上下文优先于协议数量

新能力通常应至少强化以下一项：

- 保留完整工程上下文；
- 关联多个会话或仪器的信息；
- 将原始字节转化为有意义的工程语义；
- 提升可重复性或自动化能力；
- 提供真正有深度的工业工作流；
- 将物理工程仪器纳入同一个工作环境。

如果某个协议不能明显强化这些目标，应优先通过插件/扩展实现，而不是无限扩大 Core。

### 3.4 优先建设 TauTerm 原生工作流

TauTerm 应优先建设自己的 Workspace、数据模型和工程工作流，而不是投入大量资源实现面向其他应用格式的迁移导入功能。

如果某种开放生态标准能够带来长期工程价值，则可以单独考虑互操作支持。

### 3.5 深入工业领域，但不缩窄产品边界

铁路和工业工程是重要的纵向领域，但 TauTerm 的横向产品定位仍然是工程工作台。

TRDP 等官方深度能力应体现专业工程深度，同时继续参与更广泛的连接系统工作流。

### 3.6 软件与仪器属于同一个平台

TauTerm 计划成为未来自研工程仪器的统一桌面软件。第一个明显候选是未来的 CAN 分析仪，之后还可能扩展到更多分析仪。

每一种仪器都应进入同一套数据与工作流模型，而不是为每种硬件重新开发一套彼此割裂的桌面软件。

自研硬件应获得最深度、最顺畅的集成体验；同时，架构可以在有价值时通过稳定扩展边界支持第三方或通用适配器。

## 4. 共享工程数据模型

长期产品一致性应来自统一事件管线，而不是彼此独立的功能孤岛。

```text
Transport / Instrument
        ↓
     Raw Event
        ↓
     Framing
        ↓
     Decoder
        ↓
 Structured Event / Signal
   ├─ Terminal / Packet View
   ├─ Signal Lab
   ├─ Data Lens
   ├─ Unified Timeline
   ├─ Recorder / Replay
   └─ Automation
```

这套模型最终应能够覆盖 SSH 输出、journald、Serial、TCP/UDP、TRDP、CAN 以及自研仪器。

原始信息应在足够靠近数据源的位置被保留下来，使 Recording 在未来仍可重新解码、重新分析。

## 5. 战略能力支柱

### 5.1 Foundation

**目标：** 让 TauTerm 足够稳定、顺手，能够作为工程师全天保持开启的主力工具。

优先能力包括：

- Local Shell；
- 分屏；
- SSH Tunnel 与 Jump Host；
- 从简单会话分组升级到 Workspace 基础能力；
- 长时间运行稳定性和明确的性能预算；
- 优秀的会话与配置管理体验；
- 跨平台发布质量。

这些能力负责建立主力工具所需的基础质量，并支撑后续所有高级能力。

### 5.2 Engineering Memory

**目标：** 让调试过程可以复现，而不是随着窗口关闭而消失。

TauTerm 应建设结构化 Recording/Replay，并尽量在靠近 Raw Event 的位置保留工程证据。

一次 Recording 应在适用时保存：

- 时间戳与时钟域；
- 会话/仪器身份；
- Transport 与 Peer；
- TX/RX 方向；
- 原始字节或采样；
- 已解码/结构化字段；
- Marker 与注释；
- 自动化动作；
- 文件传输与测试事件。

Replay 应允许工程师在不重新连接真实设备的情况下重新分析问题，并在条件允许时使用更新后的 Decoder 再次解码历史原始数据。

更大的目标是 **Unified Timeline**：围绕同一个工程事件，将多个会话和仪器的数据按时间关联起来。

### 5.3 Signal Lab

**目标：** 在 TauTerm 内提供完整的实时数值数据工作流。

Signal Lab 应足够强，使大量嵌入式实时数据调试场景不再需要额外打开独立实时绘图软件。

目标能力包括：

- 高吞吐实时绘图；
- 长时间采集时可控的资源占用；
- 多通道曲线；
- 缩放、游标和测量；
- FFT 与统计；
- 暂停、检查、继续；
- 数据导出；
- FireWater / JustFloat 兼容；
- 自定义 Framing 与数值提取；
- 与 Recording/Replay 数据打通。

Signal Lab 回答的问题是：

> **信号随时间发生了什么？**

### 5.4 Data Lens

**目标：** 将原始协议与设备数据转化为可以在整个产品中复用的工程语义。

一个二进制数据包可以被解释为：

```text
traction_status
├─ vehicle_id: 3
├─ speed: 61.2 km/h
├─ brake_pressure: 2.8 bar
├─ traction_current: 412 A
└─ crc: OK
```

解码后的字段最终应能够用于：

- 过滤与搜索；
- 表格与报文检查；
- 曲线与仪表；
- 统计；
- 自动化触发；
- Timeline 关联；
- 导出与报告。

Decoder 模型应能够跨 Serial、TCP/UDP、TRDP、CAN 和未来仪器复用。

Data Lens 回答的问题是：

> **这些字节意味着什么？**

Signal Lab 与 Data Lens 共用底层数据管线，但解决的是两个不同的工程问题。

### 5.5 Industrial Depth

**目标：** 在选定的工业领域中解决完整的专业工作流，而不仅仅是“支持协议”。

TRDP 是官方战略协议，因为它服务真实的铁路工程需求。

它的长期工作流可以包括：

- PD/MD 检查；
- COMID 可见性与过滤；
- Multicast/Source 分析；
- Sequence 与 Timeout 诊断；
- Cycle/Jitter/Loss 统计；
- Dataset 解码；
- Recording 与 Replay；
- 与 Serial、SSH、journald 事件关联。

其他工业协议也应按照工作流深度、工程价值以及与共享数据模型的适配程度进行评估。

### 5.6 Instrument Platform

**目标：** 让 TauTerm 成为自研分析仪的统一上位机环境。

未来 CAN 分析仪是第一个明显候选。它应当作为一等 Instrument/Session 接入，并直接复用 Workspace、Recorder、Unified Timeline、Data Lens、Signal Lab 和 Automation。

每增加一种仪器，都应自动获得现有软件平台的能力；而每增加一项软件能力，也应自然提升更多仪器的价值。

参见 [HARDWARE_ECOSYSTEM.md](HARDWARE_ECOSYSTEM.md)。

### 5.7 Automation

**目标：** 从孤立脚本演进到可重复的工程自动化流程。

Lua 继续作为重要的高级脚本运行时。未来可以在其上提供更高层的 Trigger / Condition / Action 流程，让普通用户无需写完整脚本也能构建自动化。

示例：

```text
WHEN Serial matches "READY"
THEN SSH run "./start-backend.sh"

WHEN TRDP field timeout == true
THEN mark recording
AND query journald
```

潜在层次包括：

- Lua API v2；
- Trigger / Condition / Action Flow；
- CLI 自动化；
- 带明确权限与审计能力的 MCP/Agent 接口。

Agent 能力必须遵循本地优先原则，并且不能要求工业数据必须离开用户环境。

### 5.8 Team 与 Enterprise

**目标：** 在不改变本地优先工程模型的前提下，为组织提供额外价值。

潜在能力包括：

- 共享 Workspace；
- 共享 Decoder 与自动化库；
- Recording 审阅与注释；
- 私有插件/仪器 Registry；
- 密钥与策略控制；
- 审计日志；
- 离线/Floating/Site License；
- 受控更新与 LTS；
- 企业支持。

参见 [COMMERCIALIZATION.md](COMMERCIALIZATION.md)。

## 6. 路线图模型

路线图按能力组织。版本号只是交付载体，而不是战略本身。

| 阶段 | 目标结果 |
|---|---|
| **Foundation** | 主力工具质量：Local Shell、分屏、SSH Tunnel/Jump Host、Workspace 基础、发布与性能质量 |
| **Engineering Memory** | 结构化 Recording、Replay、Marker、搜索与 Unified Timeline |
| **Signal Lab** | 高性能绘图、FFT/统计、FireWater/JustFloat 与实时数值工作流 |
| **Data Intelligence** | Framing/Decoder SDK、Data Lens、可复用字段、过滤、可视化与触发器 |
| **Industrial & Instruments** | 深入 TRDP 工作流、离线/长稳工业能力，以及未来 CAN 分析仪等自研仪器接入 |
| **Automation & Teams** | Flow 自动化、Lua/CLI/MCP 演进、协作、治理和企业部署 |

这些阶段描述方向，不构成版本交付承诺。

## 7. 产品决策过滤器

一个新功能在成为 Core 优先项之前，应回答以下问题：

1. 它具体解决什么工程问题？
2. 它是否强化共享 Workspace 或数据模型？
3. 它是否提升观察、理解、复现或自动化能力？
4. 它是否具有广泛复用价值，还是更适合做成插件/模块？
5. 它是否保持本地优先？
6. 如果它引入新协议或新仪器，是否能在适用时参与 Recording、Timeline、Data Lens、Signal Lab 或 Automation？

## 8. 明确的非目标

除非未来出现足够强的产品证据改变方向，否则 TauTerm 不优先投入：

- 面向其他应用格式的迁移型导入器；
- 仅为了增加数量而加入协议；
- 工程功能必须登录云账号才能使用；
- 向与核心工程场景无关的通用桌面工具类别扩张；
- 无法参与共享数据、Recording 和 Automation 模型的孤立仪器 UI。

## 9. 复利式产品价值

TauTerm 的长期产品价值来自这些能力互相增强：

**本地优先工程工作流 + 远程系统 + 嵌入式设备 + 工业协议 + 自研仪器 + 结构化 Recording/Replay + Signal Lab + Data Lens + Automation。**

目标不是堆出最多的功能，而是形成一个一致的工程环境：新协议、新仪器和新分析能力都能够继续强化同一套 Workspace 与数据模型。
