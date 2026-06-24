## 1. 模块重组与基础设施

- [x] 1.1 创建 `src-tauri/src/transfer/types.rs`，将 `TransferProgress`、`BatchFileResult`、`TransferDirection`、`FileTransferEvent`、`FileInfo`、`TransferProtocolType` 从 `ymodem.rs` 和 `plugin_adapter.rs` 移至公共类型模块，添加 `YModemFileEvent` deprecated type alias
- [x] 1.2 创建 `src-tauri/src/transfer/crc.rs`，实现 `crc16_ccitt()`、`crc32_zmodem()`、`checksum()` 函数，使用 `crc` 或 `crc-any` crate（若引入）或基于 lrzsz `crctab.c` 自实现查表
- [x] 1.3 创建 `src-tauri/src/transfer/io.rs`，提取 `read_byte_with_timeout()`、`flush_port_buffer()`、`send_cancel()` 公共 I/O 函数
- [x] 1.4 创建 `src-tauri/src/transfer/protocol.rs`，定义 `TransferProtocol` trait（`send_files` + `receive_files`）并实现 `From<TransferProtocolType>` 工厂
- [x] 1.5 更新 `src-tauri/src/transfer/mod.rs`，重新导出新模块并保持旧路径兼容
- [x] 1.6 更新 `src-tauri/Cargo.toml`，添加 `crc` 或 `crc-any` 依赖（可选，用于标准化 CRC 计算）

## 2. YMODEM 按 lrzsz 标准重构

- [x] 2.1 创建 `src-tauri/src/transfer/ymodem.rs`（重写现有文件），实现 `YModem` 结构体和 `TransferProtocol` trait
- [x] 2.2 实现 YMODEM 发送器，按 lrzsz `wcs`/`wctx`/`wcputsec` 流程：等待 'C' 探测 → 块 0 元数据 → 数据块 1k/128B 自适应（≤896B 阈值）→ EOT→ACK 握手 → 空块 0 结束批次
- [x] 2.3 实现 YMODEM 接收器，按 lrzsz `wcrx`/`wcgetsec` 流程：发送 'C' 探测 → 解析块 0 元数据（name\0size\0mtime\0mode\0\0）→ 数据块 CRC 验证 + 重复包检测 → EOT 处理 → 空块 0 结束批次
- [x] 2.4 保留现有前端事件发射逻辑（TransferProgress、FileTransferEvent、BatchFileResult），适配新的 `TransferProtocol` trait 接口

## 3. XMODEM 协议实现

- [x] 3.1 创建 `src-tauri/src/transfer/xmodem.rs`，定义 `XModem` 结构体（含变体标识：Standard/Crc/OneK）
- [x] 3.2 实现 XMODEM 发送器，按 lrzsz `wcs`/`wctx` 流程：等待 NAK/C/G 探测 → 自适应变体选择 → 数据块发送（128B/1k + checksum/CRC）→ 不足块 0x1A 填充 → EOT→ACK 握手
- [x] 3.3 实现 XMODEM 接收器，按 lrzsz `wcrx` 流程：发送启动字符（NAK/C/G）→ 解析 SOH/STX 块头 → checksum/CRC 验证 → 重复包检测 → EOT 处理
- [x] 3.4 为 `XModem` 实现 `TransferProtocol` trait（注意：XMODEM 仅单文件，batch send 取第一个文件，receive 接收单个文件）

## 4. ZMODEM 协议实现

- [x] 4.1 创建 `src-tauri/src/transfer/zmodem.rs`，定义 `ZModem` 结构体和 ZMODEM 常量（对齐 lrzsz `zmodem.h`）
- [x] 4.2 实现 ZMODEM 帧编解码基础函数：`zsbhdr()`/`zshhdr()`（二进制/十六进制帧头发送）、`zgethdr()`（帧头接收）、`zsdata()`/`zsda32()`（数据帧发送，含 ZDLE 转义）、`zrdata()`（数据帧接收，含 ZDLE 反转义）、`stohdr()`/`rclhdr()`（偏移量编码）
- [x] 4.3 实现 ZMODEM 发送状态机（对齐 lrzsz `zsendfile`/`wcsend`）：ZRQINIT 发起 → 等待 ZRINIT → ZFILE 文件信息 → ZDATA 滑动窗口数据流 → ZEOF 文件结束 → ZFIN 批次结束 → `OO` 退出
- [x] 4.4 实现 ZMODEM 接收状态机（对齐 lrzsz `rzfile`/`wcreceive`）：等待 ZRQINIT → 发送 ZRINIT（能力声明）→ 接收 ZFILE 文件信息 → ZDATA 滑动窗口接收 → ZEOF/ZFIN 处理 → `OO` 退出
- [x] 4.5 实现滑动窗口流控：ZCRCG（连续帧）/ZCRCQ（查询确认）/ZCRCW（窗口关闭），窗口参数对齐 lrzsz 默认值（`Txwindow=1400`）
- [x] 4.6 实现自适应块大小：从 1024B 启动，成功传输后逐步增至 8192B，错误时回退至 1024B（对齐 lrzsz `calc_blklen`）
- [x] 4.7 实现断点续传（crash recovery）：ZRPOS 帧发送/处理，接收方检查已存在文件大小，发送方从指定偏移量继续
- [x] 4.8 为 `ZModem` 实现 `TransferProtocol` trait

## 5. 命令层与前端集成

- [x] 5.1 更新 `src-tauri/src/commands.rs`：重构 `send_files_ymodem`/`receive_files_ymodem` 命令为 `send_files`/`receive_files`（接受 `protocol: TransferProtocolType` 参数），通过 `TransferProtocol` trait 工厂创建对应协议处理器
- [x] 5.2 更新 `src-tauri/src/channel/io_loop.rs`（若涉及）：`IoLoopCmd::TransferPort` 适配新协议选择参数
- [x] 5.3 更新 `src-tauri/src/channel/serial_channel.rs`（若涉及）：`try_handoff()` 返回的端口交出逻辑保持不变，协议选择通过新参数传递
- [x] 5.4 更新 `src-tauri/src/lib.rs` 中的命令注册，将 `send_files_ymodem`/`receive_files_ymodem` 替换为 `send_files`/`receive_files`
- [x] 5.5 前端事件名保持兼容：后端继续 emit `transfer-progress`、`transfer-file-start`、`transfer-file-complete`、`transfer-complete`，payload 增加 `protocol` 字段
- [x] 5.6 更新 `TransferManager::select_strategy_by_protocol()` 以适配新的 `TransferProtocolType` 引用路径

## 6. 测试

- [x] 6.1 编写 `crc.rs` 单元测试：验证 CRC-16/CCITT（已知测试向量：`"123456789"` → `0x29B1`）、CRC-32（已知测试向量：`"123456789"` → `0xCBF43926`）、checksum
- [ ] 6.2 编写 `io.rs` 单元测试：使用 `Cursor<Vec<u8>>` 模拟串口验证 `read_byte_with_timeout`、`flush_port_buffer`、`send_cancel`
- [ ] 6.3 编写 YMODEM 单元测试：使用内存 `Cursor` 模拟双方收发，验证块 0 元数据编解码、数据块 CRC 校验、EOT 握手序列、批量文件传输
- [ ] 6.4 编写 XMODEM 单元测试：使用内存 `Cursor` 模拟标准/CRC/1k 三种变体收发
- [ ] 6.5 编写 ZMODEM 单元测试：使用内存 `Cursor` 验证帧编解码（ZDLE 转义往返）、ZRQINIT/ZRINIT 协商、数据帧滑动窗口、断点续传偏移量
- [ ] 6.6 使用实际嵌入式设备（RT-Thread U-Boot）进行集成测试：YMODEM 发送/接收、XMODEM 发送/接收、ZMODEM 发送/接收
