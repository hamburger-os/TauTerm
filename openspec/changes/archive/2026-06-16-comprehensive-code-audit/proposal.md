## Why

对 TauTerm v0.2 代码库进行全面审计后发现：2 个严重 YModem 功能缺陷（传输立即自取消 + 接收不写文件）、约 8 个文件的前端/后端死代码、终端搜索功能未完成、以及多处代码质量与架构问题。这些问题影响核心功能的可靠性、可维护性和代码整洁度，需要在功能迭代前优先修复。

## What Changes

- **修复关键 Bug**：YModem 传输取消通道竞态（send_files_ymodem / receive_files_ymodem 完全不可用）、YModemReceiver 不写文件、Toast 双重自动关闭竞态
- **完成未完工功能**：SearchBar 集成 xterm.js 搜索插件实现实际查找高亮与导航
- **清理死代码**：移除 `useFileTransfer`、`useSerialPort`、`SerialConfigSidebar` 前端死代码；移除 `serial::manager`、`transfer::protocol`、`SerialConfig`、`YModem` 旧结构体等后端死代码
- **修复代码质量**：消除 `as any` 类型断言、统一快捷键 Action ID 为枚举、修复 ConnectDialog 返回按钮标签、O(n) 索引查找优化
- **架构优化**：精简 `TermSession` trait 为具体枚举类型、将 `tokio` 依赖从 `full` 降为仅需功能、引入自定义错误类型消除字符串错误、将魔法数字提取为命名常量
- **补充错误处理**：为 Tauri `emit` / `listen` 调用添加错误日志、为连接对话框添加 Escape 关闭

## Capabilities

### New Capabilities
- `codebase-cleanup`: 移除前后端所有死代码和未使用依赖，保持代码库整洁
- `error-handling-hardening`: 引入类型化错误、补充缺失的错误日志与边界处理
- `architecture-polish`: 精简 TermSession trait、优化 Mutex 策略、减小 tokio 依赖体积

### Modified Capabilities
- `file-transfer`: 修复 YModem 发送自取消 Bug 和接收不写文件 Bug（**BREAKING** 行为变更：传输从不可用到正常工作）
- `term-search`: 搜索栏从仅计数改为实际高亮定位（功能性变更：搜索从占位变为可用）
- `session-manager`: TermSession trait 重构为具体类型，取消通道从局部变量提升为 SessionHandle 字段

## Impact

- **Rust 后端**: `commands.rs`（取消通道生命周期）、`session/serial.rs`（接收写文件）、`session/manager.rs`（TermSession 重构、取消通道字段）、`session/mod.rs`（trait → enum）、`transfer/ymodem.rs`（共享 read_byte_with_timeout）、`lib.rs`（移除未使用模块）
- **TypeScript 前端**: `App.tsx`（Toast 竞态修复）、`Terminal/SearchBar.tsx`（xterm 搜索集成）、`Layout/ConnectDialog.tsx`（按钮标签）、`CommandPalette/CommandPalette.tsx`（索引优化）、`Layout/BottomInfoPanel.tsx`（类型安全）
- **移除文件**: `src/hooks/useFileTransfer.ts`、`src/hooks/useSerialPort.ts`、`src/components/Sidebar/SerialConfigSidebar.tsx` 及其 CSS 模块；`src-tauri/src/serial/manager.rs`、`src-tauri/src/transfer/protocol.rs`
- **依赖变更**: `tokio` features 从 `["full"]` 缩减为 `["rt", "sync"]`；移除 `thiserror`（或开始实际使用）
- **无破坏性 API 变更**（前端 Tauri invoke/event 接口保持不变）
