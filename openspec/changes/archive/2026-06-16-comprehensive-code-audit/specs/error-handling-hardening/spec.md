# error-handling-hardening

## Purpose

补充前后端关键路径上缺失的错误处理，将字符串错误升级为类型化错误枚举，消除不安全的类型断言。

## ADDED Requirements

### Requirement: Typed Error Enums (Backend)
系统 SHALL 定义 `TauTermError` 枚举替代 `Result<_, String>` 返回类型，覆盖以下错误变体：
- `SerialPortNotFound(String)` — 端口不存在
- `SerialPortBusy(String)` — 端口被占用
- `SerialPortDisconnected(String)` — 连接断开
- `IoError(std::io::Error)` — 通用 I/O 错误
- `TransferError(String)` — 文件传输错误
- `SessionNotFound(String)` — 会话不存在
- `SessionLimitReached` — 达到最大会话数
- `InvalidParams(String)` — 无效参数

#### Scenario: Frontend receives typed error
- **WHEN** 连接不存在的串口 "COM99"
- **THEN** 返回的错误包含可区分的错误类型标识，前端根据类型显示合适的用户提示

#### Scenario: YModem transfer error is specific
- **WHEN** YModem 传输因 CRC 错误在第 3 次重试后失败
- **THEN** 返回 `TauTermError::TransferError` 包含具体描述，前端可据此显示重试次数和失败原因

### Requirement: Tauri Event Emission Error Logging (Backend)
所有 `app.emit()` 调用 SHALL 记录失败情况。使用 `log::warn!` 替代 `let _ = app.emit()` 静默丢弃。

#### Scenario: Event emission fails gracefully
- **WHEN** Tauri 事件发送失败（如应用关闭中）
- **THEN** 系统记录 warn 级别日志，不影响主流程执行

### Requirement: Tauri Event Listener Error Handling (Frontend)
所有 `listen()` 调用 SHALL 附加 `.catch()` 处理器记录错误。

#### Scenario: Event listener setup fails
- **WHEN** `listen('session-data', ...)` 返回 rejected promise
- **THEN** 错误被 `console.error` 记录，应用继续运行而不崩溃

### Requirement: Type-Safe i18n Key Access
`BottomInfoPanel.tsx` 中的 `t(`connectionType.${activeTab.connection_type}` as any)` SHALL 被替换为类型安全的实现，使用 `connectionType` 的显式 `as_str()` 方法或类型收窄。

#### Scenario: No as any in i18n usage
- **WHEN** TypeScript 严格模式编译
- **THEN** 项目中不存在因 i18n 键访问导致的 `as any` 类型断言

### Requirement: Escape Key Closes ConnectDialog
ConnectDialog SHALL 响应 Escape 键关闭对话框，与 CommandPalette 和 ContextMenu 保持一致。

#### Scenario: Escape closes connect dialog
- **WHEN** ConnectDialog 打开且用户按下 Escape
- **THEN** 对话框关闭，焦点返回主窗口

### Requirement: Toast Dismiss Race Condition Fix
Toast 的自动关闭逻辑 SHALL 仅存在于一处（Toast 组件本身），App.tsx 中的重复超时逻辑 SHALL 被移除。

#### Scenario: Toast dismisses exactly once
- **WHEN** Toast 显示 5 秒后
- **THEN** `onClose` 回调精确触发一次，Toast 从列表中移除，不产生重复关闭或竞态警告
