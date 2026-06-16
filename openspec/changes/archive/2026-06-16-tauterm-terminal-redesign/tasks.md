## 1. Rust Backend — SessionManager 架构

- [x] 1.1 创建 `src-tauri/src/session/manager.rs`，实现 `SessionManager` 结构体（`sessions: HashMap`, `active_id`, `tab_order`）和 `SessionHandle`
- [x] 1.2 实现 `SessionManager::create_session()` — 创建 SerialSession、启动 I/O 线程、返回 session ID
- [x] 1.3 实现 `SessionManager::close_session()` — 停止 I/O 线程、释放端口、清理资源
- [x] 1.4 实现 `SessionManager::switch_active()` — 切换活跃标签页
- [x] 1.5 实现 `SessionManager::rename_session()` 和 `reorder_tabs()`
- [x] 1.6 更新 `AppState` — 用 `Mutex<SessionManager>` 替代 `Mutex<SerialSession>`

## 2. Rust Backend — I/O 修复

- [x] 2.1 将 `SerialSession::spawn_io_thread()` 中的 `mpsc::channel()` 改为 `mpsc::sync_channel(32)`
- [x] 2.2 重写 I/O 循环为读写公平交替调度（移除 `continue` 跳过写检查）
- [x] 2.3 将 tick interval 从 10ms 降为 1ms
- [x] 2.4 在每次循环迭代中处理所有排队写操作（`while let Ok(cmd) = write_rx.try_recv()` 替代 `match` 单条）

## 3. Rust Backend — Tauri Commands 重构

- [x] 3.1 更新 `connect_session` — 路由到 `SessionManager::create_session()`，返回 session ID
- [x] 3.2 更新 `disconnect_session` — 接受 `session_id` 参数，关闭指定会话
- [x] 3.3 更新 `write_data` — 接受 `session_id` 参数，写入指定会话
- [x] 3.4 新增 `switch_active_session` 命令 — 切换活跃标签页
- [x] 3.5 新增 `rename_session` 命令 — 重命名会话
- [x] 3.6 新增 `reorder_tabs` 命令 — 标签页排序
- [x] 3.7 更新文件传输命令 — 适配新的 SessionManager 架构
- [x] 3.8 更新 Tauri events — `session-data` 事件携带 `session_id`

## 4. Rust Backend — Session 持久化

- [x] 4.1 创建 `src-tauri/src/session/store.rs`，实现 `SessionStore` 结构体
- [x] 4.2 实现 `SessionStore::save()` — 序列化所有会话配置到 JSON 文件
- [x] 4.3 实现 `SessionStore::load()` — 从 JSON 文件恢复会话配置
- [x] 4.4 实现损坏文件回退逻辑 — 备份 .bak + 返回空列表
- [x] 4.5 新增 Tauri 命令 `save_sessions` 和 `load_sessions`
- [x] 4.6 在 SessionManager 的 create/close/rename 操作后自动调用 save

## 5. Frontend — 基础设施

- [x] 5.1 安装 `framer-motion` 依赖
- [x] 5.2 创建 `src/context/SessionContext.tsx` — 使用 `useReducer` 管理会话列表、活跃标签页、连接状态
- [x] 5.3 创建 `src/context/ThemeContext.tsx` — 管理当前主题、主题列表、主题切换
- [x] 5.4 创建 `src/context/TransferContext.tsx` — 管理文件传输状态
- [x] 5.5 重构 `src/hooks/useSerialPort.ts` → `src/hooks/useSession.ts` — 适配新的多会话架构
- [x] 5.6 创建 `src/hooks/useKeyboard.ts` — 全局快捷键监听 hook

## 6. Frontend — AppShell 布局

- [x] 6.1 创建 `src/components/Layout/AppShell.tsx` — 顶层布局容器，包裹所有 Context Provider
- [x] 6.2 创建 `src/components/Layout/QuickConnectBar.tsx` — 顶部快速连接栏（协议选择 + 地址输入 + Connect 按钮）
- [x] 6.3 创建 `src/components/Layout/SessionSidebar.tsx` — 左侧会话列表（搜索 + 列表 + 添加按钮）
- [x] 6.4 创建 `src/components/Layout/TabBar.tsx` — 标签页栏（标签渲染、拖拽排序、+ 按钮、关闭按钮）
- [x] 6.5 创建 `src/components/Layout/StatusBar.tsx` — 底部状态栏（连接状态、Rx/Tx 计数器、快捷操作按钮）
- [x] 6.6 创建 `src/components/Layout/ResizeHandle.tsx` — 可拖拽分割线组件（鼠标接近检测 + 发光效果）

## 7. Frontend — 终端视图

- [x] 7.1 重构 `src/components/Terminal/Terminal.tsx` — 接受 `sessionId` prop，注册对应 session 的数据事件
- [x] 7.2 创建 `src/components/Terminal/TerminalView.tsx` — 管理多个 Terminal 实例，根据活跃标签页显示/隐藏
- [x] 7.3 实现标签页切换时的 `AnimatePresence` 过渡动画
- [x] 7.4 实现 xterm.js 实例的复用和清理逻辑（关闭标签页时 dispose）

## 8. Frontend — 终端搜索

- [x] 8.1 创建 `src/components/Terminal/SearchBar.tsx` — 搜索覆盖层（输入框 + 匹配计数 + 导航按钮 + 大小写开关）
- [x] 8.2 实现 `xterm.js` addon 集成 — 使用 `addons/search` 或手动实现 buffer 搜索
- [x] 8.3 实现高亮渲染和导航滚动

## 9. Frontend — 命令面板

- [x] 9.1 创建 `src/components/CommandPalette/CommandPalette.tsx` — 模态覆盖层 + 搜索输入 + 命令列表
- [x] 9.2 实现模糊搜索算法（简单的子串匹配 + 评分，或使用 fuse.js）
- [x] 9.3 注册所有可用命令（Session, Terminal, Transfer, Theme, Application 分类）
- [x] 9.4 实现命令执行分发（调用对应的 Context action 或 Tauri invoke）

## 10. Frontend — 快捷键系统

- [x] 10.1 创建 `src/shortcuts/registry.ts` — 快捷键注册表（id → {keys, description, action}）
- [x] 10.2 注册所有默认快捷键（Ctrl+Shift+N, Ctrl+Shift+W, Ctrl+F, Ctrl+Shift+P 等）
- [x] 10.3 实现冲突检测 — 重复注册时 warn
- [x] 10.4 在 `useKeyboard` hook 中集成快捷键匹配和分发

## 11. Frontend — 文件传输增强

- [x] 11.1 创建 `src/components/FileTransfer/Dropzone.tsx` — 拖拽检测覆盖层
- [x] 11.2 实现全局 `dragenter`/`dragleave`/`drop` 事件监听
- [x] 11.3 实现 Dropzone 动画序列：窗口变暗 → 面板呼吸闪烁 → 扫光 → 进度条
- [x] 11.4 更新 `FileTransferPanel.tsx` — 集成 Dropzone、流光进度条

## 12. Frontend — Liquid Glass Theme v2

- [x] 12.1 重写 `src/styles/tokens.css` — 扩展 3x token 体系，支持 `[data-theme="neon-dark"]`
- [x] 12.2 创建 Ocean Blue 和 Sunset Amber 主题变量
- [x] 12.3 创建 `src/components/common/GlassPanel.tsx` v2 — 使用 Framer Motion `whileHover` 实现发光边框
- [x] 12.4 创建 `src/components/common/GlassButton.tsx` v2 — hover 光晕、click 涟漪效果
- [x] 12.5 创建 `src/components/common/StatusDot.tsx` — 呼吸灯动画 + 连接成功涟漪
- [x] 12.6 实现全局主题切换逻辑 — `document.documentElement.dataset.theme = '...'`

## 13. Frontend — i18n 更新

- [x] 13.1 更新 `zh-CN.json` — 添加 `session`、`search`、`palette`、`shortcuts`、`theme`、`layout` 命名空间
- [x] 13.2 更新 `en-US.json` — 添加对应英文翻译
- [x] 13.3 更新 `src/i18n/types.ts` — 扩展类型定义

## 14. 集成测试和 Bug 修复

- [ ] 14.1 测试串口回车键响应 — 验证修复后 100ms 内响应
- [ ] 14.2 测试多标签页 — 创建/切换/关闭/重命名
- [ ] 14.3 测试会话持久化 — 保存、重启恢复、损坏文件回退
- [ ] 14.4 测试终端搜索 — 大小写、导航、无结果
- [ ] 14.5 测试命令面板 — 模糊搜索、命令执行
- [ ] 14.6 测试文件传输 — Dropzone 拖拽、YModem 收发、取消
- [ ] 14.7 测试主题切换 — 3 套主题之间平滑切换
- [ ] 14.8 测试布局响应式 — 窗口缩放、侧边栏拖拽、面板拖拽
- [ ] 14.9 测试快捷键 — 所有默认快捷键触发正确操作
- [ ] 14.10 测试 i18n — 中英文切换完整性

## 15. 收尾

- [x] 15.1 更新 README.md — 新截图、新功能列表、新架构图
- [x] 15.2 清理未使用的旧代码 — `serial/manager.rs` (SerialPortManager) 标记 deprecated 或移除
- [x] 15.3 移除旧 CSS 中不再使用的样式
- [x] 15.4 更新 `tauri.conf.json` — 窗口标题、最小尺寸调整
