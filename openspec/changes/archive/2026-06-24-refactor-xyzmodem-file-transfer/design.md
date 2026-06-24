## Context

TauTerm 当前仅有手写的 YMODEM 实现（`ymodem.rs`，~985 行），XMODEM 和 ZMODEM 在 `TransferProtocolType` 枚举中已声明但未实现。现有 YMODEM 实现在握手时序、CRC 计算、EOT 重传逻辑等方面与 lrzsz 标准存在差异，可能导致与某些嵌入式设备的兼容性问题。

lrzsz 是 X/Y/ZModem 协议的事实标准参考实现，被 RT-Thread、U-Boot、Zephyr、BusyBox 等几乎所有嵌入式系统采用。按 lrzsz 逻辑实现可最大化设备兼容性。

Rust 生态中无同时覆盖三种协议的成熟库（`rzsz` 是 CLI 工具非库，`xmodem`/`ymodem`/`rmodem` 各缺部分协议），因此需自实现协议逻辑。

**约束**: 必须保持现有前端事件 API 兼容（`TransferProgress`、文件事件、批次结果），I/O 层通过 `Box<dyn serialport::SerialPort>` 操作串口。

## Goals / Non-Goals

**Goals:**
- 按 lrzsz `wcrx`/`wcs`/`wcputsec`/`wcgetsec` 标准逻辑重构 YMODEM 收发，确保嵌入式设备兼容性
- 实现完整 XMODEM 协议（标准/1k/CRC 变体），支持单文件收发
- 实现完整 ZMODEM 协议（二进制帧、滑动窗口、32 位 CRC、断点续传），支持批量文件收发
- 定义 `TransferProtocol` trait 统一三种协议接口
- 提取共享 CRC/校验和计算、I/O 工具函数到独立模块
- 保持前端事件 API 向后兼容

**Non-Goals:**
- 不引入第三方 modem crate 作为主要实现（它们不完整或不可靠）
- 不修改前端 UI 组件（仅后端协议层变更）
- 不实现 ZMODEM 的 LZW 压缩、RLE 编码、加密等可选扩展
- 不修改 SSH/SFTP/SCP 传输路径
- 不改变 `TransferManager` 的策略选择逻辑（仅扩展协议选项）

## Decisions

### Decision 1: 自实现协议逻辑，不依赖第三方 crate

**选择**: 基于 lrzsz-0.12.20 源代码，在 Rust 中重新实现 X/Y/ZModem 协议逻辑。

**理由**:
- Rust 生态中无同时覆盖三种协议的成熟库：`rzsz` 是 CLI 二进制（非库），`xmodem`（仅 XMODEM）、`ymodem`（X+Y）、`rmodem`（仅 X，Y/Z 计划中）、`txmodems`（仅 X，早期阶段）
- 自行实现可精确控制协议行为，确保与 lrzsz 标准一致
- 三个协议共享大量基础组件（CRC、I/O、块结构），合并实现减少重复
- 可引入 `crc` 或 `crc-any` crate 用于标准化 CRC 计算，避免手写查表代码

**备选方案**: 使用 `rzsz` 作为子进程调用 — 被否决，因为子进程管理复杂，无法获取逐块进度事件，且 `rzsz` 不支持作为库使用。

### Decision 2: 模块化文件结构

**选择**: 采用扁平文件结构，每个协议一个 `.rs` 文件 + 共享模块：

```
src-tauri/src/transfer/
├── mod.rs          # 公开 API，re-export
├── manager.rs      # TransferManager（基本不变）
├── types.rs        # 公共类型：TransferProgress, BatchFileResult, FileTransferEvent, ProtocolType
├── protocol.rs     # TransferProtocol trait 定义
├── crc.rs           # CRC-16/CCITT, CRC-32, 校验和计算
├── io.rs           # I/O 工具：超时读取(read_byte_with_timeout)、缓冲区刷新、CAN 发送
├── xmodem.rs       # XModem sender + receiver
├── ymodem.rs       # YModem sender + receiver（重构自现有实现）
└── zmodem.rs       # ZModem sender + receiver（全新实现）
```

**理由**: 每个协议模块 ~500-800 行，扁平结构比深层目录更易导航；共享基础组件提取后避免重复。

### Decision 3: TransferProtocol trait 设计

**选择**: 定义 `TransferProtocol` trait 统一三种协议的收发接口：

```rust
pub trait TransferProtocol: Send + Sync {
    fn send_files(
        port: &mut Box<dyn serialport::SerialPort>,
        files: &[FileInfo],
        on_progress: &dyn Fn(TransferProgress),  // 使用 &dyn 而非泛型以支持 trait object
        on_file_event: &dyn Fn(FileTransferEvent),
        cancel: &mut dyn FnMut() -> bool,
    ) -> Result<Vec<BatchFileResult>, Box<dyn std::error::Error>>;

    fn receive_files(
        port: &mut Box<dyn serialport::SerialPort>,
        download_dir: &str,
        on_progress: &dyn Fn(TransferProgress),
        on_file_event: &dyn Fn(FileTransferEvent),
        cancel: &mut dyn FnMut() -> bool,
    ) -> Result<Vec<BatchFileResult>, Box<dyn std::error::Error>>;
}
```

**理由**: 使用 `&dyn Fn` + `&mut dyn FnMut()` 支持 trait object（通过 `Box<dyn TransferProtocol>` 动态分发），无需泛型传染。`Send + Sync` 约束支持跨线程使用。

### Decision 4: YMODEM 关键行为对齐 lrzsz

**选择**: 严格按 lrzsz `wcs`（发送）和 `wcrx`（接收）流程实现：

- **发送方块大小适应**: 当剩余字节 ≤ 896 时切换为 128 字节块（`wctx` 逻辑）
- **EOT 握手**: 发送 EOT → 等待 ACK（lrzsz 原始逻辑，非 NAK→EOT→ACK 序列）
- **接收方 CRC 模式协商**: 发送 'C'（0x43）而非 NAK 启动 CRC 模式
- **块 0 元数据格式**: `filename\0size\0mtime_octal\0mode_octal\0\0`（128 字节块）
- **数据块填充**: 不足块以 `0x1A`（Ctrl-Z）填充
- **重复包检测**: 接收方跟踪 `last_block_num`，收到重复块时 ACK 但不写入

**理由**: 这些行为是 lrzsz 与嵌入式设备端实现的兼容性基础，偏离会导致与 RT-Thread ymodem 组件等主流实现握手失败。

### Decision 5: ZMODEM 帧编码架构

**选择**: 参考 lrzsz `zm.c` 的帧处理逻辑，实现三种帧格式：

- **二进制帧 (ZBIN)**: `ZPAD ZDLE ZBIN type f3 f2 f1 f0 data* crc1 crc2 ZDLE ZCRCE|ZCRCG|ZCRCQ|ZCRCW`
- **32 位 CRC 二进制帧 (ZBIN32)**: 同上但使用 CRC-32
- **十六进制帧 (ZHEX)**: `ZPAD ZPAD ZHEX type_hex f3_hex f2_hex f1_hex f0_hex crc1_hex crc2_hex CR LF [XON]`

ZMODEM 收发状态机基于 lrzsz 的 `zsendfile`/`rzfile` 函数实现，包括：
1. ZRQINIT/ZRINIT 能力协商
2. ZFILE 文件信息交换
3. ZDATA 数据帧滑动窗口传输
4. ZEOF/ZFIN 结束握手
5. ZRPOS 断点续传位置协商

**理由**: lrzsz 的帧格式和状态机是 ZMODEM 协议的唯一权威实现参考。

### Decision 6: 事件类型重命名策略

**选择**: `YModemFileEvent` 重命名为 `FileTransferEvent`，保持字段结构不变。旧的 `YModemFileEvent` 保留为 type alias 并在 1-2 版本后移除。

**理由**: 新事件类型适用于所有三种协议，type alias 提供向后兼容过渡期。

## Risks / Trade-offs

- **[风险] 自实现协议复杂度和正确性**: ZMODEM 协议复杂（~1000+ 行状态机），自行实现可能引入 bug → **缓解**: 严格逐行对照 lrzsz 源码实现，编写基于已知测试向量的 CRC 单元测试，使用实际嵌入式设备（RT-Thread/U-Boot）进行集成测试
- **[风险] 前端 API 破坏性变更**: `YModemFileEvent` 重命名为 `FileTransferEvent` → **缓解**: 提供 type alias 过渡期；前端仅需更新事件名引用，无逻辑变更
- **[风险] XMODEM 变体选择歧义**: XMODEM 有三种变体（标准/1k/CRC），远程设备可能期望特定变体 → **缓解**: 发送方默认使用 CRC 模式（'C' 探测），发送方在 `getnak` 阶段自适应接收方请求的变体
- **[权衡] 代码量增加**: 从 ~985 行增加到 ~3000-4000 行 → 但逻辑清晰度、可维护性和兼容性显著提升；共享模块减少重复

## Open Questions

- ZMODEM 的 `ZCOMMAND` 和 `ZSTDERR` 帧类型是否需要在首版支持？（lrzsz 默认禁用远程命令执行）→ 建议首版不实现，标记为后续迭代
- 是否需要为三种协议提供独立的超时/重试配置参数？→ 建议使用合理默认值（对齐 lrzsz），暂不暴露配置
