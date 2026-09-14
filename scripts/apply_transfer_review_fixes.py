from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def patch(path: str, old: str, new: str, count: int = 1) -> None:
    target = ROOT / path
    text = target.read_text(encoding="utf-8")
    actual = text.count(old)
    if actual != count:
        raise RuntimeError(f"{path}: expected {count} occurrence(s), found {actual}: {old[:100]!r}")
    target.write_text(text.replace(old, new, count), encoding="utf-8")


# XMODEM checksum is the arithmetic sum modulo 256, not a two's-complement byte.
patch(
    "src-tauri/src/transfer/crc.rs",
    "pub fn checksum_verify(data: &[u8], expected: u8) -> bool {\n    checksum(data).wrapping_add(expected) == 0\n}",
    "pub fn checksum_verify(data: &[u8], expected: u8) -> bool {\n    checksum(data) == expected\n}",
)
patch(
    "src-tauri/src/transfer/crc.rs",
    "    #[test]\n    fn test_checksum() {\n        let data = [1u8, 2, 3, 4, 5];\n        let sum = checksum(&data);\n        assert_eq!(sum, 15); // 1+2+3+4+5 = 15\n    }",
    "    #[test]\n    fn test_checksum() {\n        let data = [1u8, 2, 3, 4, 5];\n        let sum = checksum(&data);\n        assert_eq!(sum, 15); // 1+2+3+4+5 = 15\n        assert!(checksum_verify(&data, 15));\n        assert!(!checksum_verify(&data, (0u8).wrapping_sub(15)));\n    }",
)
patch(
    "src-tauri/src/transfer/xmodem.rs",
    "        XModemCheckMode::Checksum => {\n            let sum = crc::checksum(data);\n            packet.push((0u8).wrapping_sub(sum));\n        }",
    "        XModemCheckMode::Checksum => {\n            packet.push(crc::checksum(data));\n        }",
)
patch(
    "src-tauri/src/transfer/xmodem.rs",
    "                Some(EOT) => break 'read_header EOT,\n                Some(other) => {",
    "                Some(EOT) => break 'read_header EOT,\n                Some(CAN) => return Err(\"发送方取消了传输\".into()),\n                Some(other) => {",
)

# Contract assertions distinguish implementation tokens from the explanatory comment.
patch(
    "scripts/test-file-transfer-lifecycle.mjs",
    "assert.doesNotMatch(xmodem, /g_mode|G.*1K|1K.*G/);",
    "assert.doesNotMatch(xmodem, /\\bg_mode\\b|const G:\\s*u8|XModemVariant|OneK/);\nassert.match(xmodem, /`G` 不代表 XMODEM-1K/);\nassert.match(xmodem, /XModemCheckMode::Checksum[\\s\\S]*packet\\.push\\(crc::checksum\\(data\\)\\)/);\nassert.match(xmodem, /Some\\(CAN\\) => return Err\\(\"发送方取消了传输\"\\.into\\(\\)\\)/);",
)

# Documentation must describe the architecture that actually exists.
patch(
    "src-tauri/src/transfer/mod.rs",
    "//! - `protocol` — 串口 SerialTransferProtocol trait 和协议工厂",
    "//! - `protocol` — 串口 `SerialTransferProtocol` trait 与 `TransferIo` 契约",
)

doc_path = ROOT / "docs/modules/TRANSFER.md"
doc = doc_path.read_text(encoding="utf-8")
english = '''## Role-aware modem configuration\n\nThe transfer command boundary uses a tagged `protocolOptions` object rather than flattened modem fields. Sending and receiving are separate Rust/TypeScript unions so a setting owned by one local role cannot silently affect the opposite role. There is no compatibility parser for the removed `blockSize`/checksum/streaming top-level request shape.\n\n- **YMODEM**: the sender may choose 128/1024-byte data blocks; the receiver follows the protocol handshake and does not reuse that sender preference.\n- **XMODEM**: sender block size (SOH=128, STX=1024) is independent of checksum negotiation. The receiver controls whether it requests CRC16 with `C`, checksum with `NAK`, or starts in automatic CRC16-with-fallback mode. `G` is not an XMODEM-1K selector.\n- **ZMODEM**: the receiver advertises CRC32 capability with `ZRINIT.CANFC32`. Sender `auto` uses CRC32 only when advertised, `crc16` stays on CRC16, and `crc32-required` fails when the peer cannot provide CRC32. Maximum sender data block size is a sender-side limit.\n\nInline transfer cancellation has one owner: `TransferScheduler` creates and stores the same `Arc<AtomicBool>` token consumed by the running transfer. The obsolete oneshot cancellation compatibility parameter is removed; oneshot channels that remain in the orchestrator are lifecycle start gates, not cancellation state.\n'''
chinese = '''## Modem 角色感知配置\n\n传输命令边界使用带 `protocol` 标签的 `protocolOptions`，不再把 Modem 参数平铺到请求顶层。发送与接收分别使用 Rust/TypeScript 联合类型，使只属于本机某一角色的设置无法误作用到另一角色。已经删除的顶层 `blockSize`、checksum、streaming 形态不保留兼容解析。\n\n- **YMODEM**：发送方可以选择 128/1024 字节数据块；接收方按协议握手运行，不复用发送方块大小偏好。\n- **XMODEM**：发送块大小（SOH=128、STX=1024）与校验协商相互独立。接收方用 `C` 请求 CRC16、用 `NAK` 请求 8 位 checksum，`auto` 则先请求 CRC16、超时后降级到 checksum；`G` 不是 XMODEM-1K 选择器。标准 checksum 在线路上传输数据字节算术和的低 8 位。\n- **ZMODEM**：接收方通过 `ZRINIT.CANFC32` 声明 CRC32 能力。发送方 `auto` 仅在对端声明能力时使用 CRC32，`crc16` 固定使用 CRC16，`crc32-required` 在对端不支持时明确失败；最大发送块大小属于发送方策略。`crc16-only` 的含义是不向对端声明 CRC32 能力。\n\nInline 传输取消只有一个所有者：`TransferScheduler` 创建并保存与协议循环共享的同一个 `Arc<AtomicBool>`。旧的 oneshot 取消兼容参数已经删除；编排器中仍存在的 oneshot 仅用于任务启动门闩，不代表取消状态。\n'''
if doc.count(english) != 1:
    raise RuntimeError("TRANSFER.md role-aware English section not found exactly once")
doc_path.write_text(doc.replace(english, chinese, 1), encoding="utf-8")

# Config choice controls are ordinary buttons, never implicit form submitters.
for path in [
    "src/components/FileTransfer/protocol-config/forms/YmodemConfigForm.tsx",
    "src/components/FileTransfer/protocol-config/forms/XmodemConfigForm.tsx",
    "src/components/FileTransfer/protocol-config/forms/ZmodemConfigForm.tsx",
]:
    target = ROOT / path
    text = target.read_text(encoding="utf-8")
    text = text.replace("<button\n", "<button\n              type=\"button\"\n")
    target.write_text(text, encoding="utf-8")

print("final review fixes applied")
