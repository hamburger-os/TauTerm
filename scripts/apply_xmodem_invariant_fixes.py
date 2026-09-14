from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def patch(path: str, old: str, new: str, count: int = 1) -> None:
    target = ROOT / path
    text = target.read_text(encoding="utf-8")
    actual = text.count(old)
    if actual != count:
        raise RuntimeError(f"{path}: expected {count} occurrence(s), found {actual}: {old[:120]!r}")
    target.write_text(text.replace(old, new, count), encoding="utf-8")


x = "src-tauri/src/transfer/xmodem.rs"

# XMODEM block numbers are 8-bit modulo-256 values: 1..255,0,1...
patch(
    x,
    "        // 块号 1..=255 循环（wrapping_add 处理回绕）\n        block_num = block_num.wrapping_add(1);\n        if block_num == 0 {\n            block_num = 1;\n        }",
    "        // XMODEM 块号为 8 位序号：1..255→0→1，自然回绕。\n        block_num = block_num.wrapping_add(1);",
)
patch(
    x,
    "/// 计算下一个预期的块号（1..=255 循环，跳过 0）\nfn next_block_num(current: u8) -> u8 {\n    let next = current.wrapping_add(1);\n    if next == 0 {\n        1\n    } else {\n        next\n    }\n}",
    "/// 计算下一个预期块号。XMODEM 使用 8 位序号，255 后自然回绕到 0。\nfn next_block_num(current: u8) -> u8 {\n    current.wrapping_add(1)\n}\n\n#[cfg(test)]\nmod tests {\n    use super::*;\n\n    #[test]\n    fn block_numbers_wrap_through_zero() {\n        assert_eq!(next_block_num(254), 255);\n        assert_eq!(next_block_num(255), 0);\n        assert_eq!(next_block_num(0), 1);\n    }\n}",
)

# A valid transfer starts at block 1. Do not ACK an arbitrary valid-looking starting block.
patch(
    x,
    "                    if bnum != !bnum_neg {\n                        port.write_all(&[NAK])?;\n                        port.flush()?;\n                        continue;\n                    }\n                    let block_size = if header == STX {",
    "                    if bnum != !bnum_neg || bnum != 1 {\n                        log::debug!(\n                            \"XModem RX: invalid first block number {} (expected 1), requesting retry\",\n                            bnum\n                        );\n                        port.write_all(&[NAK])?;\n                        port.flush()?;\n                        continue;\n                    }\n                    let block_size = if header == STX {",
)

# Noise is consumed one byte at a time; never purge bytes that may already contain a valid header.
patch(
    x,
    "                Some(other) => {\n                    log::debug!(\n                        \"XModem RX: unexpected byte 0x{:02X} waiting for header\",\n                        other\n                    );\n                    io::flush_port_buffer(port);\n                }",
    "                Some(other) => {\n                    log::debug!(\n                        \"XModem RX: ignoring noise byte 0x{:02X} while waiting for header\",\n                        other\n                    );\n                }",
)

# Lock these protocol invariants into the lifecycle/architecture contract.
contract = "scripts/test-file-transfer-lifecycle.mjs"
patch(
    contract,
    "assert.match(xmodem, /Some\\(CAN\\) => return Err\\(\"发送方取消了传输\"\\.into\\(\\)\\)/);",
    "assert.match(xmodem, /Some\\(CAN\\) => return Err\\(\"发送方取消了传输\"\\.into\\(\\)\\)/);\nassert.match(xmodem, /fn next_block_num\\(current: u8\\) -> u8 \\{\\s*current\\.wrapping_add\\(1\\)\\s*\\}/);\nassert.match(xmodem, /bnum != !bnum_neg \\|\\| bnum != 1/);\nassert.doesNotMatch(xmodem, /flush_port_buffer\\(port\\)/);",
)

# Document the modulo-256 sequence rule alongside the role-aware XMODEM contract.
doc = "docs/modules/TRANSFER.md"
patch(
    doc,
    "- **XMODEM**：发送块大小（SOH=128、STX=1024）与校验协商相互独立。接收方用 `C` 请求 CRC16、用 `NAK` 请求 8 位 checksum，`auto` 则先请求 CRC16、超时后降级到 checksum；`G` 不是 XMODEM-1K 选择器。标准 checksum 在线路上传输数据字节算术和的低 8 位。",
    "- **XMODEM**：发送块大小（SOH=128、STX=1024）与校验协商相互独立。接收方用 `C` 请求 CRC16、用 `NAK` 请求 8 位 checksum，`auto` 则先请求 CRC16、超时后降级到 checksum；`G` 不是 XMODEM-1K 选择器。标准 checksum 在线路上传输数据字节算术和的低 8 位；块号为 8 位序号并按 `1..255→0→1` 自然回绕。等待块头时只逐字节忽略噪声，不清空可能已经包含合法帧头的 RX 缓冲。",
)

print("xmodem final protocol invariants fixed")
