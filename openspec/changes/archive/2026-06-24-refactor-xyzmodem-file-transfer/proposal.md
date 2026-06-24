## Why

当前 YMODEM 实现是手写的，与 lrzsz 标准行为存在偏差，可能导致与某些嵌入式设备（RT-Thread、U-Boot、Zephyr 等）的兼容性问题。XMODEM 和 ZMODEM 协议已声明但未实现。需要按照 lrzsz 标准逻辑重构全部三个协议，确保与业界最广泛使用的设备端实现兼容。

## What Changes

- **重构 YMODEM 收发**：按 lrzsz `wcrx`/`wcs` 标准逻辑重写，对齐块大小适应（896 字节阈值）、EOT 重传握手、CRC 模式协商等标准行为
- **新增 XMODEM 支持**：实现标准 XMODEM（128B 块 + 校验和）、XMODEM-1k（1024B 块 + CRC-16）、XMODEM-CRC（128B 块 + CRC-16）三种变体
- **新增 ZMODEM 支持**：实现基于 ZMODEM 规范的收发，包括 ZRQINIT/ZRINIT 初始化协商、ZDLE 转义编码、滑动窗口流控、32 位 CRC 支持、自适应块大小（最大 8KB）、断点续传
- **统一协议抽象**：定义 `Protocol` trait 解耦协议逻辑与 I/O，支持 `Box<dyn serialport::SerialPort>` 作为传输层
- **共享 CRC/校验基础设施**：提取 CRC-16/CCITT、CRC-32、校验和计算到独立模块，为所有协议复用
- **保持现有事件/进度 API 兼容**：`TransferProgress`、`YModemFileEvent`（重命名为 `FileTransferEvent`）、`BatchFileResult` 等类型保持向后兼容

## Capabilities

### New Capabilities

- `xmodem-file-transfer`: XMODEM 文件收发协议实现（标准/XMODEM-1k/XMODEM-CRC 变体），包括基于校验和和 CRC-16 的错误检测、128B/1KB 块大小、EOT 握手
- `zmodem-file-transfer`: ZMODEM 文件收发协议实现，包括二进制帧（ZBIN/ZBIN32/ZDLE 转义）、滑动窗口流控、32 位 CRC、自适应块大小、断点续传、ZRQINIT/ZRINIT 能力协商
- `transfer-protocol-abstraction`: 协议无关的传输抽象层，定义 `Protocol` trait 统一 X/Y/ZModem 接口，共享 CRC/校验基础设施，支持 `Read + Write` 通用传输后端

### Modified Capabilities

- `file-transfer`: YMODEM 收发逻辑按 lrzsz 标准重构，扩展为支持 X/Y/ZModem 三种协议选择
- `ymodem-batch-error-recovery`: 重构后的批量错误恢复逻辑需适配新的 `Protocol` trait 架构，错误恢复行为保持一致
- `ymodem-batch-progress`: 进度事件 API 对前端保持兼容，`TransferProgress` 等类型扩展支持 XModem/ZModem

## Impact

- Affected code: `src-tauri/src/transfer/` (全部文件)、`src-tauri/src/commands.rs` (传输命令)、`src-tauri/Cargo.toml` (新增依赖)
- Dependencies: 可能引入 `crc` 或 `crc-any` crate 用于标准化 CRC 计算；协议逻辑完全自实现（不引入不完整的第三方 modem crate）
- Breaking change: `YModemFileEvent` 重命名为 `FileTransferEvent`，前端 event 监听需同步更新
- I/O 线程: `IoLoopCmd::TransferPort` 和相关 handoff 逻辑升级以支持三种协议选择
