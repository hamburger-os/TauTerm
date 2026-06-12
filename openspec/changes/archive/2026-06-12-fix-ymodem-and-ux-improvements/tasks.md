## 1. YModem 发送修复

- [x] 1.1 修改 `YModemSender::send()` — 在发送批次结束空块 0 时捕获错误，若文件数据块和 EOT 均已成功则降级为 `log::warn!` 而非返回 `Err`
- [x] 1.2 确认 `transfer-complete` 事件在空块 0 失败时仍正确发送 `success:true`
- [ ] 1.3 测试：连接开发板，执行 `ry` 接收模式，选择小文件（<10KB）发送并验证进度条完成后提示成功而非"块0收到意外响应"

## 2. 工具栏组件

- [x] 2.1 新建 `src/components/Layout/Toolbar.tsx` — 渲染 `ToolbarButton` 组件：New Session（+）、Refresh（⟳）、Transfer（⬆）、Sidebar（☰）、Commands（⌘），每个包含图标+文字
- [x] 2.2 新建 `src/components/Layout/Toolbar.module.css` — 水平 flexbox、按钮间隔、hover 效果、active 状态样式
- [x] 2.3 在 `App.tsx` 中将 `<QuickConnectBar>` 替换为 `<Toolbar>`，传入现有回调（`setConnectDialogOpen`、`togglePanel`、`setSidebarVisible`、`setPaletteOpen`、`refreshEndpoints`）
- [x] 2.4 删除 `src/components/Layout/QuickConnectBar.tsx` 和 `QuickConnectBar.module.css`

## 3. 会话持久化 — 后端

- [x] 3.1 在 `commands.rs` 中添加 `save_sessions` Tauri 命令：获取 `SessionManager` 锁 → 调用 `save_to_disk()`，路径使用 `SessionManager::sessions_file_path(app_handle)`
- [x] 3.2 在 `commands.rs` 中添加 `load_sessions` Tauri 命令：调用 `SessionManager::load_from_disk()`，返回 `Vec<SavedSession>`
- [x] 3.3 在 `lib.rs` 中注册 `save_sessions` 和 `load_sessions` 命令
- [x] 3.4 在 `main.rs` 或 `lib.rs` 的 `tauri::Builder` 中添加 `on_event` 钩子，监听 `RunEvent::Exit` 事件时调用 `save_to_disk`

## 4. 会话持久化 — 前端

- [x] 4.1 在 `SessionContext` 初始化 `useEffect` 中调用 `load_sessions` 命令，将返回的 `SavedSession[]` 注入 `tabs` state（状态标记为 `"disconnected"`）
- [x] 4.2 在 `TabInfo` 类型中保存 `endpoint` 和 `params` 字段，确保恢复的标签页可回填配置到 `SerialConfigSidebar`
- [x] 4.3 点击恢复的标签页时，`SerialConfigSidebar` 回填保存的串口参数（endpoint、baud_rate 等）

## 5. 文件传输面板默认高度

- [x] 5.1 在 `App.tsx` 中将 `togglePanel` 打开时的默认高度从 `200` 提取为常量 `PANEL_DEFAULT = 200`，确保首次打开即可看到完整面板内容
- [x] 5.2 验证：面板关闭时高度回到 `PANEL_MIN`（24px），打开时跳至 `PANEL_DEFAULT`（200px）

## 6. i18n 翻译

- [x] 6.1 在 `en-US.json` 中添加工具栏按钮标签：`toolbar.newSession`、`toolbar.refresh`、`toolbar.transfer`、`toolbar.sidebar`、`toolbar.commands`
- [x] 6.2 在 `zh-CN.json` 中添加对应中文标签：新建会话、刷新端口、文件传输、侧栏、命令面板

## 7. 最终集成与清理

- [x] 7.1 运行 `cargo build` 确认后端编译无错误
- [x] 7.2 运行 `npm run build` 确认前端编译无错误
- [ ] 7.3 端到端验证：连接开发板 → 发送文件验证 YModem 修复 → 创建多个会话 → 关闭窗口 → 重启验证会话恢复 → 检查工具栏按钮功能
