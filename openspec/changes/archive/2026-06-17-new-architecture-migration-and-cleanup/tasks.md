## 1. Backend Module Reference Verification

- [x] 1.1 验证 `src-tauri/src/lib.rs` 中所有 `mod` 声明均为新架构模块（`channel`、`commands`、`kernel`、`plugins`、`security`、`transfer`），确认不存在 `mod serial;` 或 `mod session;`
- [x] 1.2 验证 `src-tauri/src/` 各子模块 `mod.rs` 中的子模块声明均指向现有文件（`channel/mod.rs` → `error`/`io_loop`/`serial_channel`、`kernel/mod.rs` → 10 个子模块、`plugins/mod.rs` → `serial` 等）
- [x] 1.3 运行 `cargo build` 确认后端编译零错误、零警告（除 `channel/mod.rs` 中的 dead_code 警告将在步骤 4 处理）

## 2. Frontend Import Chain Verification

- [x] 2.1 对 `src/` 目录执行 grep 搜索已删除文件引用（`useSerialPort`、`useFileTransfer`、`SerialConfigSidebar`、`GlassSelect`），确认零匹配
- [x] 2.2 验证所有组件 import 均指向新架构路径（`core/plugin-registry`、`renderers/TerminalRenderer`、`plugins/serial`、`context/SessionContext`）
- [x] 2.3 运行 `npm run build` 确认前端 TypeScript 编译零错误

## 3. Empty Directory Cleanup

- [x] 3.1 删除空目录 `src-tauri/src/serial/`（旧串口模块，功能已迁移至 `plugins/serial/` 和 `channel/`）
- [x] 3.2 删除空目录 `src/components/Sidebar/`（`SerialConfigSidebar` 已删除，功能迁移至 ConnectDialog）
- [x] 3.3 删除空目录 `src/components/Layout/ConnectDialog/`（`ConnectDialog.tsx` 是扁平文件，不需要子目录）
- [x] 3.4 再次运行 `cargo build` 和 `npm run build`，确认空目录删除不影响构建

## 4. Architecture Stub Type Documentation

- [x] 4.1 修改 `src-tauri/src/channel/mod.rs`：移除 `IoStrategy` 枚举的 `#[allow(dead_code)]`，添加 doc comment 说明 `Sync` 用于串口、`Async` 为 SSH/TCP 预留
- [x] 4.2 修改 `src-tauri/src/channel/mod.rs`：移除 `ContentType` 枚举及 `impl` 块中所有 `#[allow(dead_code)]`，添加 doc comment 说明各变体对应的前端渲染器
- [x] 4.3 运行 `cargo build`，确认无 dead_code 警告（pub 导出枚举变体被编译器视为"已使用"）

## 5. Toolbar Layout Alignment

- [x] 5.1 验证 `src/components/Layout/Toolbar.tsx` 的三区布局（`leftZone`/`centerZone`/`rightZone`）正确实现了全局按钮和插件按钮的分发
- [x] 5.2 验证 `App.tsx` 中 `handleToolbarAction` 处理 4 个 actionId（`newSession`、`sidebar`、`commands`、`settings`），与 Toolbar 按钮的 `onClick` 回调一致
- [x] 5.3 验证 Serial 插件通过 `plugins/serial/index.ts` 注册的 toolbarItems 使用了正确的 `position` 属性

## 6. Final Integration Verification

- [x] 6.1 运行 `cargo build` 确认后端编译成功（零错误、零警告）
- [x] 6.2 运行 `npm run build` 确认前端编译成功（零错误）
- [x] 6.3 检查 `git status` 确认仅有预期的变更（提案文档 + 清理的文件），无不跟踪的孤立文件残留
