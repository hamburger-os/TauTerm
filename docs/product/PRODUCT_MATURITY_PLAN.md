# Product Integrity & Daily Driver Plan

> 本文是 TauTerm 在下一轮大功能开发前的执行计划。它把产品战略中的长期方向转化为当前研发阶段可验证的工程门槛；版本号只是交付载体。

## 1. 为什么先做完整性收敛

TauTerm 的连接能力已经覆盖 SSH/SFTP、Serial、Local Shell、TCP/UDP、TFTP、Telnet、iperf 与 TRDP。后续仍可以根据真实工程需求、用户价值和行业深度少量新增协议，但协议数量不作为阶段完成度或竞争力的主要指标。下一阶段更需要强化：

- 全天候主力工具的稳定性和操作效率；
- 可复用、可导出、可复现的工程 Workspace；
- 不静默丢失的工程证据链；
- 统一的 Event/Framing/Decoder 数据底座；
- 跨 Session / Instrument 的关联分析与自动化。

在 Recording、Data Lens、Signal Lab、CAN/Instrument 等能力进入 Core 之前，先消除多份 source of truth、临时持久化和不可观测丢失，可以显著降低后续重构成本。

## 2. Product Integrity baseline（本轮完成）

### 2.1 Trust Contract

本轮已落实：

- Tauri updater signing 与 Windows Authenticode 的当前状态在 workflow、CHANGELOG、平台/安全文档中一致；
- System Log 与 Session Data Log 使用独立启用语义；
- 日志/显示缓冲丢失必须计数并可观察；
- SSH host key 信任使用持久 known-host 模型；已知主机密钥变化默认拒绝；
- 生产 SSH 连接路径不存在“缺少 verifier 就自动接受”的 fallback。

### 2.2 Plugin Contract

本轮已落实：

- Plugin manifest / capabilities 只有一个 canonical 定义；
- 前后端从同一份 manifest 消费或由 CI 证明完全一致；
- Core 不再维护与运行路径重复的“影子插件状态”。

Connect Form、默认命名、reconnect preflight 等剩余协议语义继续按实际收益逐步回到插件边界，但不再作为 Product Integrity 的阻塞项；迁移时必须避免再制造第二份 manifest/capability。

### 2.3 Persistence Contract

本轮已落实：

- 持久化文件必须有明确 schema version；
- Saved Session 与 runtime Session 分离；
- Workspace Layout 的持久化 owner 已从 WebView 本地状态收敛到 Rust ConfigStore，并正式定义完整 TauWorkspace 资产模型；
- 当前已存在的 Command Set、Script、Auto Reply 有明确 global ownership；Highlight、Decoder、Recording Profile 在各自能力落地时必须直接进入同一资产模型，不再新增临时存储；
- 安全凭据只保存 reference，永不进入 Workspace export。

### 2.4 Observability Contract

TauTerm 明确区分：

```text
Presentation Path                    Evidence Path
I/O → batching → terminal UI         I/O → recorder/event store
      可在过载时受控降级                   不允许静默制造完整记录
      丢失必须可观察                       gap/overflow 必须成为显式事件
```

普通 Session Log 是诊断/文本日志；未来 Recorder 是工程证据系统，两者不能通过重命名互相替代。

### 2.5 Frontend Quality Gate

至少自动覆盖：

- 1 / 横向 2 / 纵向 2 / 2×2 Pane；
- Terminal、Network、TFTP、iperf、TRDP Node/Monitor/Analysis；
- 无不可达主操作、无意外 document overflow、无重复同轴滚动、Pane header 几何一致；
- source-level UI contract 与 split-layout 不变量进入 CI；视觉/E2E smoke 在能够稳定启动 WebView fixture 后继续扩展。

### 2.6 Dead Architecture Cleanup

每一个 Core service 必须满足二选一：

1. 它是当前运行时 canonical owner；或
2. 删除。

不保留“未来可能使用”的平行 TabHost/WindowManager/IPC 等骨架，避免架构文档和实际运行路径发生漂移。

## 3. 当前 0.6.x 收敛边界

在启动下一轮大功能开发前，0.6.x 继续承担已有能力的成熟度收敛。当前阶段允许：

- 修复正确性、安全、资源生命周期和持久化问题；
- 补齐已有功能的错误处理、状态反馈和交互一致性；
- 提升长时间运行、断线/重连、异常退出和高负载场景的稳定性；
- 建立可重复的性能基线、诊断能力和 CI 防回归门；
- 打磨现有 SSH、Serial、Network、Transfer、TRDP、Automation、Workspace Layout 等已交付能力；
- 在有明确工程价值时少量新增协议，但不得以“凑协议数量”替代当前成熟度工作，也不得为新增协议复制第二套 Session、持久化、发送或插件模型；
- 当 0.6.x 正在修改 Transport / Session 的底层 I/O、source identity、时间戳或 overflow 语义时，可以顺手把边界设计成未来 EngineeringEvent 可承接的形态，但不在 0.6.x 提前建设 Recording、Data Lens、Signal Lab、Named Workspace 等下一阶段大功能。

0.6.x 的目标是让当前产品更可信、更稳定、更容易诊断，而不是提前消耗下一阶段的功能预算。

## 4. Daily Driver Gate

Product Integrity 完成且 0.6.x 收敛达到预期后，下一阶段优先提升日常主力工具成熟度：

### Session Library

- Saved Session 与 Active Session 规模分离（Integrity baseline 已完成）；
- Folder / Tag / Favorite / Recent；
- Quick Open / 搜索；
- 批量连接、批量编辑与模板。

### Serial Maturity

- USB device identity（VID/PID/serial/manufacturer/product）元数据采集（Integrity baseline 已完成）；
- 基于 stable identity 的热插拔与 same-device reconnect；
- DTR / RTS / BREAK 控制；
- CTS / DSR / RI / CD 状态；
- 明确时间戳与 framing 入口。

### SSH Maturity

- persistent known-host trust（Integrity baseline 已完成）；
- OpenSSH config interoperability；
- SSH Agent；
- ProxyJump；
- Local / Remote / Dynamic forwarding。

### Terminal Maturity

- Regex search；
- persistent highlight profiles；
- Workspace/Session 级规则覆盖。

## 5. Data Foundation

Data Foundation 不要求等待所有 Daily Driver 项目全部结束后才开始。面向用户的大功能仍按阶段控制，但只要某个 Transport / Session 正在进行底层重构，就应优先建立与 EngineeringEvent 兼容的 source identity、时间、方向、raw payload 和 gap/overflow 边界，减少未来 Recorder/Data Lens 的二次重构。共享数据底层成熟后，再建设高级 UI。

```text
Transport / Instrument
        ↓
 EngineeringEvent
        ↓
 Raw payload + timestamp + clock domain
        ↓
 Framing
        ↓
 Decoder
        ↓
 Structured Event / Signal
```

EngineeringEvent 至少应覆盖：

- stable source/session identity；
- wall + monotonic timestamp；
- clock domain；
- peer/transport；
- TX/RX；
- raw payload；
- event type；
- metadata；
- optional structured payload；
- explicit gap/overflow markers。

## 6. 后续能力顺序

```text
Product Integrity
      ↓
0.6.x Maturity Closure
      ↓
Daily Driver ──────┐
      ↓             │ 可在底层渐进铺设
Data Foundation ◀──┘
      ↓
Engineering Memory
Recording / Replay / Marker
      ↓
Unified Timeline
      ↓
Signal Lab + Data Lens
      ↓
Automation Flow
      ↓
Industrial Depth / Instrument Platform
```

Data Lens 的底层 Framing/Decoder 应早于 Signal Lab 高级 UI，避免 Serial/TRDP/TCP/CAN 各自复制 parser。

## 7. 版本边界原则

- 不为了版本号强行塞入无关功能；
- 0.x 开发阶段允许清理旧开发数据格式，不维护无价值迁移分支；
- 每次 minor 版本应有一个清晰的用户结果，而不是协议数量 KPI；协议扩展保持开放，可根据真实需求少量新增；
- v1.0 前优先消除影响长期数据模型、安全边界和可复现性的技术债。

## 8. Done 定义

一次“完成”至少同时满足：

- 代码只有一个权威状态/配置来源；
- Rust/TypeScript/文档契约一致；
- CI 有防回归检查；
- 错误与数据丢失可观察；
- 运行时资源有显式生命周期；
- 用户工作区中的工程资产可以解释其所有权和持久化位置。
