## Context

TauTerm 是一个基于 Tauri + React + TypeScript 的串口终端应用。当前分支 `master` 已具备：多标签页会话管理、YModem 文件传输、侧栏串口配置、命令面板等核心功能。用户在实际连接开发板测试后，报告了 4 个影响日常使用的体验问题。此设计文档覆盖所有 4 个修复/改进的技术方案。

现有架构：
- **前端**：React Context（SessionContext、TransferContext）管理状态，AppShell 包装 Provider
- **后端**：`SessionManager` 持有 `HashMap<TabId, SessionHandle>` 管理会话生命周期，已内置 `SavedSession` 序列化结构和 `save_to_disk`/`load_from_disk` 方法
- **传输**：`YModemSender::send()` 分三阶段——等待 'C'、逐文件发送（块0元数据→数据块→EOT）、批次结束空块0

## Goals / Non-Goals

**Goals:**
- 修复 YModem 单文件传输完成后误报"块0收到意外响应"的错误
- 将顶部快速连接栏替换为功能工具栏（图标+文字），提供新建会话、刷新端口、切换侧栏、传输面板、命令面板 5 个快捷操作
- 实现应用关闭时自动保存会话配置、启动时恢复标签页（不自动重连）
- 文件传输面板默认打开高度从 24px 提升到 200px

**Non-Goals:**
- YModem 接收流程修复（当前问题仅在发送端）
- 工具栏自定义/拖拽排序
- 会话自动重连（仅恢复标签页，用户手动点击连接）
- 跨设备会话同步

## Decisions

### D1: YModem 空块0 — 降级为警告而非致命错误

**选择**：在 `YModemSender::send()` 中，发送批次结束空块 0 时捕获错误，仅记录 warn 日志并通过 `transfer-complete` 事件通知前端传输成功。

**替代方案**：移除空块 0 发送 → 拒绝。部分 YModem 接收端（如 lrzsz）期望空块 0 作为批次结束信号，保留发送逻辑但容错处理更稳妥。

**理由**：单文件传输时，EOT + ACK 后数据已完整传输。部分嵌入式 YModem 实现在 EOT 后立即退出协议模式，不再响应空块 0。降级处理确保真实传输完成不被误报为失败。

### D2: 工具栏组件 — 新建 Toolbar 替代 QuickConnectBar

**选择**：新建 `src/components/Layout/Toolbar.tsx`，渲染 5 个 `ToolbarButton`（图标+短标签）。从 `App.tsx` 移除 `<QuickConnectBar>`，替换为 `<Toolbar>`。按钮通过回调触发现有操作（`setConnectDialogOpen`、`togglePanel`、`setSidebarVisible`、`setPaletteOpen`、`refreshEndpoints`）。

**替代方案**：使用 TabBar 区域整合 → 拒绝。TabBar 已有标签页切换职责，混入功能按钮会使 TabBar 复杂化且视觉拥挤。

**理由**：工具栏作为独立的一行，视觉上与标签页行分离，符合主流终端应用（iTerm2、Windows Terminal）的布局惯例。

### D3: 会话持久化 — 利用现有 SessionManager 基础设施

**选择**：在 `commands.rs` 中添加两个 Tauri 命令：
- `save_sessions`：调用 `SessionManager::save_to_disk()`，在应用关闭前由前端调用（或通过 `tauri::RunEvent::Exit` 钩子）
- `load_sessions`：调用 `SessionManager::load_from_disk()`，启动时返回 `Vec<SavedSession>` 给前端恢复标签页 UI

前端 `SessionContext` 在初始化时调用 `load_sessions`，将返回的 `SavedSession` 列表注入 `tabs` state（状态标记为 `disconnected`）。在 `App` 组件中通过 `useEffect` 的 cleanup 或 `window.onbeforeunload` 触发 `save_sessions`。

**替代方案**：Tauri plugin-store → 拒绝。`SessionManager` 已有完整的保存/加载逻辑，不引入新依赖更简洁。

**理由**：基础设施已就绪（`SavedSession`、`save_to_disk`、`load_from_disk`），只需串联生命周期钩子。

### D4: 文件传输面板默认高度

**选择**：将 `App.tsx` 中 `panelHeight` 的初始值从 `PANEL_MIN`（24）改为 `200`，同时 `isPanelOpen` 初始值保持 `false`。当 `togglePanel()` 打开时设置 200，关闭时回到 `PANEL_MIN`。

**理由**：24px 的初始高度对于包含按钮、进度条、历史列表的面板过于拥挤。200px 是 `togglePanel` 已使用的值，保持一致且无需额外常量。

## Risks / Trade-offs

- **[YModem] 空块0 失败误吞真实错误** → 仅对单文件传输且 EOT 已 ACK 的场景降级；多文件传输中非最后文件的空块 0 仍报错
- **[工具栏] 未来按钮增多可能导致溢出** → 预留 `overflow: hidden` + `flex-wrap` 策略，按钮数量控制在 ≤7 个
- **[会话持久化] 损坏的 sessions.json 导致启动白屏** → `load_from_disk` 已包含损坏文件备份+返回空列表的逻辑，前端容错处理空列表
- **[面板高度] 小屏幕设备 200px 占用终端面积** → 用户可拖拽缩小，`PANEL_MIN` 仍为 24px 允许完全收起
