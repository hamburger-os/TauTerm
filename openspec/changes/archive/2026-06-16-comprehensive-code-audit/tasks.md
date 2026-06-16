## 1. 死代码清理 (Codebase Cleanup)

- [x] 1.1 删除前端死代码文件：移除 `src/hooks/useFileTransfer.ts`、`src/hooks/useSerialPort.ts`、`src/components/Sidebar/SerialConfigSidebar.tsx` 及对应 CSS Module
- [x] 1.2 删除后端死代码模块：移除 `src-tauri/src/serial/manager.rs`（deprecated SerialPortManager）和 `src-tauri/src/transfer/protocol.rs`（未使用的 FileTransferProtocol trait）
- [x] 1.3 移除 `serial::config` 中未使用的 `SerialConfig` 结构体及其 `#[allow(dead_code)]` 注解
- [x] 1.4 更新 `lib.rs` 模块声明：移除 `serial::manager` 和 `transfer::protocol` 的 `pub mod` 声明；移除过时的 re-export
- [x] 1.5 执行 `npm run build` 验证前端编译通过，无引用错误
- [x] 1.6 执行 `cargo build` 验证后端编译通过，无未使用导入警告

## 2. YModem 关键 Bug 修复 (File Transfer)

- [x] 2.1 修复取消通道竞态（Bug #1）：在 `SessionHandle` 中增加 `cancel_transfer_tx: Option<tokio::sync::oneshot::Sender<()>>` 字段；修改 `send_files_ymodem` 命令将 sender 存入 SessionHandle 而非立即 drop
- [x] 2.2 修改 `cancel_transfer` 命令：从 `SessionHandle.cancel_transfer_tx.take()` 获取 sender 并发送取消信号；传输完成/取消后将字段置为 None
- [x] 2.3 修复 YModemReceiver 不写文件（Bug #2）：在 `ymodem_receive` 函数中添加 `std::fs::File::create()` 和 `file.write_all(&data)` 调用，将 CRC 校验通过的数据块写入磁盘
- [x] 2.4 重构 `read_byte_with_timeout` 为共享模块级函数，消除 `YModemSender` 和 `YModemReceiver` 中的重复实现
- [x] 2.5 提取 YModem 硬编码常量：`YMODEM_MAX_RETRIES = 10`、`YMODEM_BLOCK_SIZE = 1024` 等

## 3. 架构优化 (Architecture Polish)

- [x] 3.1 将 `TermSession` trait 重构为 `SessionImpl` 枚举：`enum SessionImpl { Serial(SerialSession) }`，移除 trait 的 stub connect/write/disconnect 方法
- [x] 3.2 更新 `SessionHandle` 使用 `SessionImpl` 替代 `Box<dyn TermSession>`
- [x] 3.3 更新 `SessionManager::create_session()` 直接构造 `SessionImpl::Serial`，移除 trait 调用的中间层
- [x] 3.4 提取后端全局命名常量：`PORT_OPEN_RETRIES`、`PORT_OPEN_RETRY_DELAY_MS`、`PORT_STABILIZE_DELAY_MS`、`IO_THREAD_TICK_MS`、`SESSION_CLOSE_DELAY_MS` 等，放置于各模块顶部
- [x] 3.5 将 `tokio` features 从 `["full"]` 缩减为 `["rt", "sync"]`；验证 `cargo build` 通过

## 4. 错误处理加固 (Error Handling Hardening)

- [ ] 4.1 定义 `TauTermError` 枚举（使用 `thiserror::Error` derive）覆盖所有错误变体
- [ ] 4.2 将 `commands.rs` 所有 `Result<_, String>` 替换为 `Result<_, TauTermError>`；更新 `impl From<TauTermError> for String` 或实现 `Serialize` 以兼容 Tauri IPC 返回
- [ ] 4.3 将 `session/` 模块所有字符串错误替换为 `TauTermError`
- [ ] 4.4 将 `transfer/ymodem.rs` 的错误返回替换为 `TauTermError::TransferError`
- [x] 4.5 将 `lib.rs` 和 `session/manager.rs` 中所有 `let _ = app.emit(...)` 替换为带 `log::warn!` 的错误处理

## 5. 终端搜索完成 (Term Search)

- [x] 5.1 重写 `SearchBar.tsx` 搜索逻辑：使用 xterm.js buffer API (`terminal.buffer.active`) 逐行扫描匹配文本
- [x] 5.2 实现匹配高亮：通过 xterm.js decorations API 或临时 marker 高亮所有匹配项，active match 使用不同的高亮颜色
- [x] 5.3 实现匹配导航：Enter 跳转下一个匹配（`terminal.scrollToLine()`），Shift+Enter 跳转上一个，到达末尾回绕到第一个
- [x] 5.4 搜索关闭时清除所有高亮 decorations

## 6. 前端质量修复 (Frontend Quality)

- [x] 6.1 修复 `BottomInfoPanel.tsx` 中的 `as any` 类型断言：使用 `ConnectionType` 的枚举值映射或类型收窄
- [x] 6.2 修复 Toast 双重关闭竞态：移除 `App.tsx` 中的重复 `setTimeout` 过滤逻辑，仅保留 Toast 组件内部的 `onClose` 回调
- [x] 6.3 修复 `ConnectDialog.tsx` 返回按钮标签：将第二步的返回按钮从 `common.cancel` 改为 `common.back`
- [x] 6.4 为 ConnectDialog 添加 Escape 键关闭支持
- [x] 6.5 优化 `CommandPalette.tsx` 索引查找：将 `flatList.indexOf(cmd)` 的 O(n) 查找替换为预计算 `Map<string, number>`
- [x] 6.6 为所有 `listen()` 调用（SessionContext、TransferContext）添加 `.catch()` 错误处理
- [x] 6.7 将快捷键 Action ID 字符串（`"session.new"`、`"sidebar.toggle"` 等）提取为 `export const enum ShortcutAction` 类型定义

## 7. 验证与测试

- [x] 7.1 `npm run build` — 前端 TypeScript + Vite 构建零错误
- [x] 7.2 `cargo build` — 后端 Rust 编译零错误、零警告
- [ ] 7.3 手动测试：串口枚举与连接/断开（基本功能回归）
- [ ] 7.4 手动测试：YModem 文件发送（使用虚拟串口对或环回）
- [ ] 7.5 手动测试：终端搜索高亮与导航
- [ ] 7.6 手动测试：Toast 正常显示和关闭、ConnectDialog Escape 关闭、命令面板搜索
- [ ] 7.7 `cargo clippy` — 无新警告
