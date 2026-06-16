## Context

TauTerm v0.2 经历了 5 轮迭代重构（会话管理架构迁移、内存管理优化、串口重连修复等），积累了未清理的过渡代码。当前代码库处于重构中间状态：旧 `SerialPortManager` 被标记 deprecated 但未移除，`TermSession` trait 的 stub 方法暴露了未完成的抽象设计。此次审计发现了关键功能 Bug（YModem）和多处代码质量问题。

项目为单仓库结构，Rust 后端在 `src-tauri/`，React 前端在 `src/`，通过 Tauri v2 IPC 通信。无测试基础设施。

## Goals / Non-Goals

**Goals:**
- 修复 YModem 文件传输使其端到端可用
- 完成终端搜索功能（从占位到实际可用）
- 清理所有死代码，消除维护混乱
- 引入类型化错误，消除 `as any` 和不安全类型断言
- 精简架构：`TermSession` trait → `SessionImpl` enum
- 缩小 tokio 依赖体积

**Non-Goals:**
- 不添加新功能（SSH/Telnet、分屏、插件系统等）
- 不引入测试框架（可在后续独立 change 中处理）
- 不重构前端状态管理（SessionContext/TransferContext 模式不变）
- 不改变 Tauri IPC 接口（命令签名和事件格式保持不变）

## Decisions

### D1: TermSession trait → SessionImpl enum

**选择**: 将 `pub trait TermSession` 替换为 `pub enum SessionImpl { Serial(SerialSession) }`

**替代方案及排除原因**:
- *保持 trait 并实现 connect/write*: 当前 `SessionManager::create_session` 直接 match `ConnectionType::Serial` 并调用 `SerialSession::create_session`，不通过 trait，因此实现 trait stub 方法无意义。trait 的真正价值在 SSH/Telnet 实现后才体现，过早抽象增加复杂度。
- *完全内联到 SessionManager*: 失去模块化，SessionImpl enum 保留了未来扩展点。

**理由**: Enum 方式比 trait 对象更直接（无 vtable 开销），更符合当前"仅有 Serial"的现实，未来变体添加清晰明确。

### D2: 取消通道生命周期

**选择**: 在 `SessionHandle` 中增加 `cancel_transfer_tx: Option<tokio::sync::oneshot::Sender<()>>` 字段

**替代方案及排除原因**:
- *在命令函数中保持 `_cancel_tx` 存活*: 命令函数返回后 `_cancel_tx` 被 drop，无法跨 Tauri 命令调用取消。Tauri 命令是无状态的（每次调用独立执行），无法在两次命令调用间共享局部变量。
- *使用全局 HashMap 存储取消通道*: 增加不必要的全局状态，SessionHandle 是自然归属。

**理由**: 取消通道的生命周期应与传输线程相同。存储于 SessionHandle 使 `cancel_transfer` 命令可以直接访问。传输开始时 `Some(tx)`，传输完成/取消时 `None`。

### D3: xterm.js 搜索实现

**选择**: 直接操作 xterm.js buffer API 进行搜索和高亮，而非引入 `@xterm/addon-search`

**替代方案及排除原因**:
- *引入 @xterm/addon-search*: npm 搜索未发现 xterm.js 5.x 官方搜索插件（`@xterm/addon-search` 在 5.x 中可能已移除或重构）。即使存在，其 API 与自定义 SearchBar UI 的集成可能受限。
- *继续 DOM 查询方案*: 当前方案无法高亮和导航，不可行。

**理由**: 使用 xterm.js 的 `terminal.buffer.active` API 进行 buffer 级别搜索，通过 decorations API（`terminal.registerDecoration`）或选区 API 实现高亮，`terminal.scrollToLine()` 实现跳转。这是 xterm.js 5.x 推荐的做法。

### D4: 错误类型

**选择**: 使用 `thiserror` 派生自定义错误枚举 `TauTermError`

**替代方案及排除原因**:
- *anyhow*: 适用于应用层，但无类型区分，不适合需要向前端传递可区分错误类型的场景。
- *手动实现 Display + Error*: 样板代码多，与已有 `thiserror` 依赖冲突。

**理由**: `Cargo.toml` 已有 `thiserror` 依赖但从未使用。用它定义错误枚举最小化样板代码，前端通过错误类型标识显示合适的用户提示。

### D5: tokio 依赖瘦身

**选择**: `tokio` features 从 `["full"]` 改为 `["rt", "sync"]`

**替代方案及排除原因**:
- *完全移除 tokio*: `tokio::sync::oneshot` 提供 `blocking_recv()`，在 `std::thread` 中使用方便。`std::sync::mpsc` 无等效的 oneshot 语义（one-shot 通过 drop sender 通知 receiver 需额外包装）。保留最小 tokio 即可。
- *引入 crossbeam-channel*: 增加新依赖，tokio oneshot 足够。

**理由**: `rt` 提供运行时基础类型（虽然未启动 runtime），`sync` 提供 oneshot 通道。移除 `net`/`io-util`/`process`/`time`/`signal` 等未使用模块可显著减少编译时间。

## Risks / Trade-offs

- **[YModem 修复后需端到端测试]**: 取消通道和文件写入修复需要通过实际硬件串口环回或虚拟串口对测试验证。→ 在实现 tasks 中列出验证步骤。
- **[Trait → Enum 的 git blame 追溯]**: 重构会改变 blame 信息。→ 在 commit message 中注明重构意图，保留 git 历史可通过分步提交。
- **[SearchBar 实现复杂度]**: xterm.js decorations API 在不同版本间可能变化。→ 锁定 `@xterm/xterm@^5.5.0`，API 参考该版本文档。
- **[死代码删除的意外引用]**: 虽然探索确认了引用关系，但未测试覆盖意味着删除可能暴露隐藏的编译路径。→ 删除后必须执行 `cargo build` 和 `npm run build` 验证。

## Migration Plan

所有变更为内部实现调整，无外部 API 变更：
1. 删除死代码 → Build 验证
2. 修复 Bug → 手动功能测试
3. 架构重构 → Build 验证 → 功能回归测试
4. 无数据迁移需求（`sessions.json` 格式不变）

回滚策略：每个 commit 独立可 revert，变更粒度控制在单文件级别。
