## 1. CRC utilities (crc.rs)

- [x] 1.1 Add `crc16_ccitt_zero_pad(data: &[u8]) -> u16` — compute CRC-16/CCITT over data, then feed two zero bytes: `updcrc(0, updcrc(0, crc))`
- [x] 1.2 Add `crc16_ccitt_feedthrough_verify(data: &[u8], crc_hi: u8, crc_lo: u8) -> bool` — feed data + two CRC bytes through CRC engine using lrzsz updcrc formula, check result == 0
- [x] 1.3 Verify both functions with round-trip, corrupt-data, and corrupt-crc tests; all pass
- [x] 1.4 Gate `crc16_ccitt_bitwise` behind `#[cfg(test)]`

## 2. I/O utilities (io.rs)

- [x] 2.1 Update `send_cancel()` to transmit 10 CAN (0x18) + 8 BS (0x08) matching lrzsz `canit()`; add port flush after write
- [x] 2.2 Add `detect_cancel(byte: u8, last_can: &mut bool) -> bool` — stateful two-CAN detector
- [x] 2.3 Add `drain_rx_buffer(port)` — read and discard bytes with 100ms timeout per byte, up to 20 bytes
- [x] 2.4 Add `probe_for_c(port, timeout_ms)` — wait for 'C' byte with configurable timeout (available for future use)

## 3. Block 0 format (ymodem.rs)

- [x] 3.1 Replace null-separated fields with lrzsz format: `{name}\0{size} {mtime} {mode:o} 0 {filesleft} {totalleft}`
- [x] 3.2 Add sector-count trailer at block0[126] and block0[127]: `let sectors = (size + 127) >> 7`
- [x] 3.3 Strip directory from filename (keep basename only) using `Path::file_name()`
- [x] 3.4 Handle overlong filenames (>125 bytes): switch to 1024-byte STX block for block 0
- [x] 3.5 Update batch-termination empty block 0: fire-and-forget send, no ACK wait

## 4. CRC wire protocol fix (ymodem.rs + xmodem.rs)

- [x] 4.1 Update `send_block()` to use `crc16_ccitt_zero_pad(data)` instead of `crc16_ccitt(data)`
- [x] 4.2 Update YModem receiver CRC check to use `crc16_ccitt_feedthrough_verify(data, crc_hi, crc_lo)`
- [x] 4.3 Fix XModem CRC — updated both sender and receiver in xmodem.rs to use zero_pad/feedthrough_verify

## 5. EOT handshake simplification (ymodem.rs)

- [x] 5.1 Rewrite `send_eot()`: send EOT, retry up to MAX_RETRIES, wait for ACK only; removed NAK→EOT→ACK, C-as-ACK, and post-ACK 'C' probe paths
- [x] 5.2 Remove post-block-0 'C' wait from `ymodem_send()` — receiver sends 'C' independently
- [x] 5.3 Inter-file sync: sender already waits for 'C' at the start of each file (Stage 1), providing natural sync for slow receivers

## 6. CAN cancel detection (ymodem.rs)

- [x] 6.1 Replace single-CAN detection in `ymodem_send()` with `detect_cancel()` stateful two-CAN detector
- [x] 6.2 Replace single-CAN detection in `ymodem_receive()` with `detect_cancel()` stateful two-CAN detector
- [x] 6.3 Update `send_block()` CAN detection to use `detect_cancel()` instead of single-byte CAN match
- [x] 6.4 Update `send_eot()` CAN detection to use `detect_cancel()`

## 7. Batch transfer fixes (ymodem.rs)

- [x] 7.1 Fix aggregate_total subtraction: moved `aggregate_total -= file_info.size` to BEFORE per-file progress loop for files that fail to open
- [x] 7.2 Add `drain_rx_buffer()` + `flush_port_buffer()` after each file in sender loop
- [x] 7.3 In receiver: after file close (EOT) and before sending 'C' for next block 0, add `drain_rx_buffer()`
- [x] 7.4 Empty block 0 batch terminator uses `let _ = send_block(...)` — fire-and-forget, no error propagation

## 8. Testing and verification

- [x] 8.1 Unit test for `crc16_ccitt_zero_pad`: self-consistency test (manual updcrc vs function)
- [x] 8.2 Unit tests for `crc16_ccitt_feedthrough_verify`: round-trip, corrupt data, corrupt CRC
- [x] 8.3 Unit tests for `detect_cancel`: double CAN → true, single CAN → false, CAN+ACK+CAN → false, no CAN → false
- [x] 8.4 Block 0 encoding: verified by code review; format matches lrzsz spec (space-separated fields, sector counts at [126-127])
- [x] 8.5 `cargo build` succeeds with 0 errors, no new warnings (11 pre-existing warnings in zmodem/manager unchanged)
- [x] 8.6 `cargo test --lib transfer` — all 21 tests pass
- [ ] 8.7 Manual integration test: send files from TauTerm to a terminal running `rb` (lrzsz receiver) — requires physical serial setup
- [ ] 8.8 Manual integration test: receive files from a terminal running `sb` (lrzsz sender) — requires physical serial setup
