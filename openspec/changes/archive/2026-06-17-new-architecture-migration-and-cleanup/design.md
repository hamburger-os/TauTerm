## Context

TauTerm v0.2 的后端架构从原始的扁平模块布局重构为微内核插件架构：

- **旧架构**：`serial/` (串口直连)、`session/` (会话管理)、`transfer/` (传输协议) 三模块交叉引用，无清晰边界
- **新架构**：`kernel/` (8 个微内核模块)、`channel/` (统一 I/O 抽象)、`plugins/` (协议插件)、`security/` (凭据与安全) — 严格分层，插件通过 `PluginHost` 注册

前端同步引入：
- `core/plugin-registry.ts` — 前端插件注册表，管理 manifest、UI 组件、翻译资源
- `renderers/` — 根据插件的 `content_type` 分发到不同渲染器
- `plugins/serial/` — Serial 插件前端注册入口

本次迁移验证确保所有新旧边界清晰，无残留文件或过期引用。

## Goals / Non-Goals

**Goals:**
- 验证后端 `lib.rs` 中所有 `mod` 声明均指向新模块（`channel`、`kernel`、`plugins`、`security`、`transfer`），不存在对 `serial/manager`、`session/manager` 等旧模块的引用
- 验证前端 `import` 链均引用 `core/plugin-registry`、`renderers/`、`plugins/` 等新模块，不存在对 `useSerialPort`、`useFileTransfer`、`SerialConfigSidebar`、`GlassSelect` 等已删除文件的引用
- 清理磁盘上残留的空目录（`src-tauri/src/serial/`、`src/components/Sidebar/`、`src/components/Layout/ConnectDialog/`）
- 确认 Toolbar 三区布局（左/中/右 + 插件注入）已正确替换旧的四按钮右侧横排布局
- 评估 `channel/mod.rs` 中 `#[allow(dead_code)]` 的类型，确保每个都有明确的未来用途注释

**Non-Goals:**
- 不新增功能特性
- 不修改 `Cargo.toml` 依赖声明（`thiserror` 已在 8 处使用，非死依赖）
- 不修改 `tokio` features（当前 `["rt", "sync", "time", "macros"]` 均有使用——`macros` 用于 `#[tokio::test]`，`time` 用于超时控制）
- 不删除 `IoStrategy` 和 `ContentType` 枚举（它们是为 SSH/TCP 插件预留的架构桩）

## Decisions

### D1: 空目录清理策略

**决策**：删除所有空目录，不保留任何"占位"目录。

**理由**：
- `src-tauri/src/serial/` 空目录：`serial/config.rs` 和 `serial/mod.rs` 已删除，功能迁移至 `plugins/serial/` 和 `channel/`
- `src/components/Sidebar/` 空目录：`SerialConfigSidebar.tsx` 已删除，功能迁移至 `ConnectDialog.tsx`
- `src/components/Layout/ConnectDialog/` 空目录：`ConnectDialog.tsx` 作为扁平文件存在于 `src/components/Layout/ConnectDialog.tsx`，空子目录是历史残留

### D2: 工具栏按钮布局验证

**决策**：确认 Toolbar 从"四按钮横排右侧"切换至"三区注入"布局。

**旧布局**：所有 4 个按钮（新建、侧栏、命令面板、设置）全部在右侧水平排列

**新布局**：
- 左区：Logo + 全局按钮（➕ 新建会话、☰ 侧栏切换）+ 插件 `position: "left"` 按钮
- 中区：插件 `position: "center"` 按钮（预留）
- 右区：插件 `position: "right"` 按钮 + 全局按钮（⌘ 命令面板、⚙ 设置）

**验证点**：
- `App.tsx` 中 `handleToolbarAction` switch 处理 4 个 actionId（`newSession`、`sidebar`、`commands`、`settings`）
- `Toolbar.tsx` 通过 `pluginRegistry` 获取活跃插件的 toolbarItems 并按 position 分发
- Serial 插件注册了 `position: "left"` 的刷新按钮

### D3: IoStrategy 和 ContentType 枚举保留

**决策**：保留 `channel/mod.rs` 中的 `IoStrategy` 和 `ContentType` 枚举，将 `#[allow(dead_code)]` 替换为文档注释说明预留用途。

**理由**：这两个枚举是为未来多协议支持预留的架构桩：
- `IoStrategy`：区分同步（串口）和异步（TCP/SSH）I/O 策略，当前仅使用 `Sync` 变体
- `ContentType`：前端渲染器分发依据，当前仅使用 `Terminal` 变体

**替代方案及其驳回理由**：
- 删除后再加：增加未来重构成本，枚举定义本身零运行时开销
- 移至未使用模块：破坏 `channel/mod.rs` 作为统一 I/O 抽象层的语义完整性

### D4: 前端模块引用验证方法

**决策**：通过 grep 扫描验证而非手动检查每个文件。

**验证项**：
1. 搜索 `from.*serial/config|from.*session/manager|use serial::|mod session` → 后端无匹配
2. 搜索 `useSerialPort|useFileTransfer|SerialConfigSidebar|GlassSelect` → 前端无匹配
3. 搜索 `import.*from.*["\'].*\/(useSerialPort|useFileTransfer|SerialConfigSidebar|GlassSelect)` → 前端无匹配

## Risks / Trade-offs

- [风险] 后端 `serial/` 空目录删除后，若某个 `git stash` 或未跟踪分支仍引用旧路径 → **缓解**：重新创建目录并还原文件即可，无数据丢失
- [风险] `IoStrategy`/`ContentType` 枚举保留可能导致读者困惑（为什么有未使用的代码） → **缓解**：添加清晰的 doc comment 说明"预留: 用于 SSH/TCP 插件"
- [权衡] 空目录删除 vs 保留 `.gitkeep` → 选择删除，因为 Git 不跟踪空目录，无需 `.gitkeep`
