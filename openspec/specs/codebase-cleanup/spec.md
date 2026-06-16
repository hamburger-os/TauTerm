# codebase-cleanup

## Purpose

移除前后端不再使用或已被替代的代码和依赖，确保代码库整洁、减少维护负担。

## Requirements

### Requirement: Frontend Dead Code Removal
系统 SHALL 移除以下未被任何组件导入的前端文件：
- `src/hooks/useFileTransfer.ts` — 已被 `TransferContext` 替代
- `src/hooks/useSerialPort.ts` — 已被 `SessionContext` 替代
- `src/components/Sidebar/SerialConfigSidebar.tsx` 及其 CSS Module — 已被 `ConnectDialog` 替代
- 删除后应用 SHALL 编译通过、功能无退化。

#### Scenario: Build succeeds after removal
- **WHEN** 上述文件被删除且执行 `npm run build`
- **THEN** TypeScript 编译和 Vite 打包成功完成，无引用错误

#### Scenario: Existing functionality unchanged
- **WHEN** 用户正常使用串口连接、文件传输、会话管理等核心功能
- **THEN** 所有现有功能行为与删除前完全一致

### Requirement: Backend Dead Code Removal
系统 SHALL 移除以下 Rust 后端死代码：
- `serial::manager` 模块（`SerialPortManager`），已标记 `#[deprecated]`，功能已迁移至 `session::manager::SessionManager`
- `transfer::protocol` 模块（`FileTransferProtocol` trait 及相关类型），YModem 已直接实现，trait 从未使用
- `serial::config` 中的 `SerialConfig` 结构体（params 通过 JSON 流转，结构体从未被读取）
- 删除后 `cargo build` SHALL 成功且所有 14 个 Tauri 命令 SHALL 保持可用。

#### Scenario: Cargo build succeeds after removal
- **WHEN** 上述死代码被删除且执行 `cargo build`
- **THEN** Rust 编译成功，无未使用导入警告、无引用错误

#### Scenario: All Tauri commands functional
- **WHEN** 前端调用任意 Tauri 命令（connect_session、send_files_ymodem 等）
- **THEN** 所有命令正常执行，行为与删除前一致

### Requirement: Unused Dependency Cleanup
系统 SHALL 移除或正确使用 Rust 依赖中未实际使用的 crate：
- `thiserror = "1"` 未被使用 → 若在错误处理改造中采用则保留并使用，否则移除
- `tokio` features 从 `["full"]` 缩减为 `["rt", "sync"]`（仅需 oneshot 通道 + 基础运行时类型）

#### Scenario: Dependencies are minimal and used
- **WHEN** `Cargo.toml` 中的依赖被审计
- **THEN** 每个声明依赖在代码中至少有一处引用，`tokio` features 不包含未使用的 `net`/`io-util`/`process` 等模块
