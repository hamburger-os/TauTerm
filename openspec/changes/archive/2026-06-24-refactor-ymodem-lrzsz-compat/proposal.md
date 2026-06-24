## Why

The current YModem implementation has protocol-level incompatibilities with the lrzsz standard (the de facto reference for YModem file transfer). Block 0 metadata format, CRC computation, CAN cancel sequences, and EOT handshake logic all diverge from lrzsz behavior. These divergences cause interoperability failures when TauTerm communicates with embedded devices (RT-Thread, U-Boot, bootloaders) and terminal emulators that implement the lrzsz wire protocol. Fixing these incompatibilities is essential for TauTerm to serve as a reliable file transfer tool in real-world embedded development workflows.

## What Changes

- **Fix Block 0 metadata format** to match lrzsz: `filename\0size mtime mode serialno filesleft totalleft` (space-separated numeric fields after single null), with sector-count bytes at positions 126-127 for IMP/KMD bootloader compatibility
- **Fix CRC-16 computation** in `send_block` to match lrzsz by zero-padding (`updcrc(0, updcrc(0, crc))`) before transmitting CRC bytes, and switch receiver to feed-through verification (`updcrc(rx_crc_hi, ...); updcrc(rx_crc_lo, ...); check == 0`)
- **Simplify EOT handshake** to match lrzsz standard: EOT → ACK only (remove the RT-Thread/U-Boot NAK→EOT→ACK variant and the speculative post-ACK 'C' probe)
- **Fix CAN cancel detection** to require two consecutive CAN bytes (matching lrzsz `Lastrx==CAN` check) instead of treating a single CAN byte as cancel — prevents false cancellation from line noise
- **Fix batch-end sequence**: after sending empty block 0, do NOT wait for ACK (matching lrzsz behavior where the empty block 0 terminates the batch without further handshake)
- **Add inter-file flush and 'C' synchronization** between files in send batch to handle slow embedded receivers that may still be processing the previous file
- **Fix aggregate progress tracking** when files fail to open (subtract from aggregate_total before the file loop, not mid-loop)
- **Add RX buffer drain after each file** in receiver to clear stale bytes before requesting next block 0
- **Restore lrzsz-standard 'C' probe flood** on receiver startup: 30 probes at 1-second intervals, matching `rb` behavior

## Capabilities

### New Capabilities

- `ymodem-lrzsz-block0-format`: Standard lrzsz Block 0 metadata encoding with space-separated fields and sector-count trailer bytes
- `ymodem-lrzsz-crc`: Standard XMODEM/YMODEM CRC-16 zero-padded transmit and feed-through verification matching lrzsz
- `ymodem-lrzsz-cancel`: Two-CAN cancel detection (sender and receiver) matching lrzsz wire protocol, plus 10-CAN+8-backspace cancel transmit

### Modified Capabilities

- `ymodem-batch-error-recovery`: Per-file error handling must use lrzsz-standard CAN cancel sequence (10 CAN + 8 BS) instead of 2 CAN; batch-end empty block 0 must not wait for ACK
- `ymodem-batch-progress`: Aggregate progress tracking fix — when a file fails to open, aggregate_total must be corrected BEFORE entering the per-file progress loop to prevent progress display glitches
- `file-transfer`: EOT handshake requirements simplified to lrzsz standard (EOT → ACK only); CAN cancel sequence requirements updated

## Impact

- **src-tauri/src/transfer/ymodem.rs** — Primary change: rewrite send_block, send_eot, ymodem_send, ymodem_receive to match lrzsz wire protocol
- **src-tauri/src/transfer/crc.rs** — Add `crc16_ccitt_verify()` for feed-through CRC verification; ensure CRC tables match lrzsz exactly
- **src-tauri/src/transfer/io.rs** — Update `send_cancel()` to emit 10 CAN + 8 backspace; add `detect_cancel()` for two-CAN detection
- **src-tauri/src/transfer/types.rs** — No structural changes expected; types already support required event payloads
- **src/context/TransferContext.tsx** — May need minor update if aggregate progress tracking fix exposes UI edge case
