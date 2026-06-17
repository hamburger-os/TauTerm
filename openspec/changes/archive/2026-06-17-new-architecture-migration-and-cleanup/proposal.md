## Why

TauTerm 刚刚经历了一次深度重构：后端从扁平模块（`serial/`、`session/` 分散管理）切换至微内核插件架构（`kernel/`、`channel/`、`plugins/`、`security/`），前端同步引入插件注册表（`core/plugin-registry`）、内容类型驱动的渲染器分发（`renderers/`）和插件化 UI 组件（`plugins/`）。然而重构过程中产生了遗留文件、空目录和不一致的工具按钮布局，存在死代码残留和新旧架构并存的风险。

## What Changes

- **新架构验证**：检查所有前端 import 链和后端 `mod` 声明是否已完全指向新模块，确认不存在对已删除模块（`serial/manager`、`session/manager`、`serial/config`、`GlassSelect`、`useSerialPort`、`useFileTransfer`、`SerialConfigSidebar`）的过期引用
- **死代码清理**：删除仍残留在磁盘上的空目录和孤立文件，包括 `src-tauri/src/serial/`、`src-tauri/src/session/` 中的残留文件，以及 `src/components/Sidebar/` 空目录
- **工具栏布局对齐**：Toolbar 从原先"四按钮横排右侧"的旧布局切换至"三区（左/中/右）+ 插件动态注入"的新布局，将 ✅ 新建会话 + ☰ 侧栏 移至左侧，⌘ 命令面板 + ⚙ 设置 保留右侧，插件按钮按 position 属性分发
- **未使用类型评估**：评估 `channel/mod.rs` 中 `#[allow(dead_code)]` 标记的 `IoStrategy` 和 `ContentType` 枚举——确认是为未来 SSH/TCP 插件预留的架构桩，本次不删除但需明确注释说明用途
- **空 ConnectDialog 目录清理**：`src/components/Layout/ConnectDialog/` 目录为空，需与现有 `ConnectDialog.tsx`（扁平文件）的关系理清，移除冗余目录

## Capabilities

### New Capabilities

- `architecture-verification`: 验证前后端模块引用完整性，确保从旧架构到新架构的迁移已完成，不存在向左依赖
- `dead-code-removal`: 清理重构后残留的空目录、孤立文件、过期模块引用

### Modified Capabilities

- `toolbar-simplification`: 工具栏按钮位置从"全部右侧横排"调整为"左/中/右三区 + 插件注入"布局——左区放全局操作（新建、侧栏）+ 插件左按钮，右区放命令面板、设置 + 插件右按钮
- `architecture-polish`: 补充验证 `IoStrategy` 和 `ContentType` 枚举的架构桩设计意图，确保其 `#[allow(dead_code)]` 有合理原因

## Impact

- **后端 (Rust)**：`lib.rs` 模块声明、`commands.rs` 命令实现、`channel/mod.rs` 类型定义
- **前端 (TypeScript/React)**：`App.tsx` 工具栏布局、`Toolbar.tsx` 按钮位置、`main.tsx` 插件注册入口、`TabContentDispatcher.tsx` 渲染器分发
- **文件系统**：清理 `src-tauri/src/serial/`、`src-tauri/src/session/`、`src/components/Sidebar/`、`src/components/Layout/ConnectDialog/` 中的空目录或残留文件
