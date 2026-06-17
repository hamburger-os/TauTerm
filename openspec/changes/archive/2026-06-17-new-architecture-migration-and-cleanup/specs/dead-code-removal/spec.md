# dead-code-removal

## Purpose

清理重构后残留在磁盘上的空目录和孤立文件，确保仓库不包含无用的旧架构残留。

## ADDED Requirements

### Requirement: Empty old-architecture directories are removed
以下空目录 SHALL 从仓库中移除，因为它们对应的源代码已迁移至新架构模块：

- `src-tauri/src/serial/` — 功能迁移至 `plugins/serial/` 和 `channel/`
- `src/components/Sidebar/` — 功能迁移至 `ConnectDialog.tsx`（扁平文件）
- `src/components/Layout/ConnectDialog/` — 空子目录，`ConnectDialog.tsx` 作为扁平文件存在于上级目录

#### Scenario: Old directories no longer exist
- **WHEN** 检查上述三个目录的存在性
- **THEN** 每个目录 SHALL 不存在于文件系统中
- **AND** `git status` 不显示这些目录下的文件变更

#### Scenario: Build succeeds after removal
- **WHEN** 上述目录被删除且执行构建
- **THEN** `cargo build` SHALL 成功（后端）
- **AND** `npm run build` SHALL 成功（前端）

### Requirement: Dead-code-annotated types are documented as reserved
`channel/mod.rs` 中的 `IoStrategy` 枚举和 `ContentType` 枚举 SHALL 保留但移除 `#[allow(dead_code)]` 注解，替换为文档注释明确说明其作为架构桩的预留用途。

#### Scenario: IoStrategy has documented purpose
- **WHEN** 开发者阅读 `channel/mod.rs` 中的 `IoStrategy` 定义
- **THEN** 枚举上方 SHALL 包含 doc comment 说明 "预留: 用于区分同步/异步 I/O 策略，当前仅使用 Sync 变体，Async 变体为 SSH/TCP 插件预留"
- **AND** 编译不产生 dead_code 警告（因各变体已在模块内被引用或通过 pub 导出）

#### Scenario: ContentType has documented purpose
- **WHEN** 开发者阅读 `channel/mod.rs` 中的 `ContentType` 定义
- **THEN** 枚举 SHALL 包含 doc comment 说明各变体对应的前端渲染器（Terminal → TerminalRenderer、FileBrowser → FileBrowserRenderer 等）
- **AND** 编译不产生 dead_code 警告

### Requirement: No residual .rs files in old module directories
`src-tauri/src/serial/` 和 `src-tauri/src/session/` 目录中 SHALL 不存在任何 `.rs` 文件。

#### Scenario: Old source files are gone
- **WHEN** 列出 `src-tauri/src/serial/` 和 `src-tauri/src/session/` 的内容
- **THEN** 两个目录都不包含 `.rs` 文件
- **AND** 如果目录为空（无 `.rs` 文件），目录本身也被删除
