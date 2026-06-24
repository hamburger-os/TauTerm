## Context

TauTerm implements YModem file transfer over serial in `src-tauri/src/transfer/ymodem.rs`. The implementation was written from the YModem specification but diverges from lrzsz-0.12.20 (the de facto reference implementation used by `sz`/`rz`/`sb`/`rb` commands) in several wire-level details. These divergences cause interoperability failures with embedded devices running lrzsz-based receivers/senders.

Key files involved:
- `src-tauri/src/transfer/ymodem.rs` — primary protocol implementation (~840 lines)
- `src-tauri/src/transfer/crc.rs` — CRC-16/CCITT tables and functions
- `src-tauri/src/transfer/io.rs` — shared I/O: CAN cancel, timeout reads, buffer flush
- `src-tauri/src/transfer/types.rs` — shared data types (no structural changes needed)

The lrzsz reference is at `C:\workspace\swos2\lrzsz-0.12.20\src\` with key functions in `lsz.c` (sender: `wcsend`, `wcs`, `wctxpn`, `wctx`, `wcputsec`, `getnak`) and `lrz.c` (receiver: `wcreceive`, `wcrxpn`, `wcrx`, `wcgetsec`, `procheader`).

## Goals / Non-Goals

**Goals:**
- Achieve wire-level compatibility with lrzsz-based YModem receivers (embedded bootloaders, `rb` command)
- Match lrzsz Block 0 metadata encoding exactly (space-separated fields, sector-count trailer)
- Match lrzsz CRC-16 computation (zero-padded transmit, feed-through verification)
- Match lrzsz CAN cancel protocol (two-CAN detection, 10-CAN+8-BS transmit)
- Simplify EOT handshake to single lrzsz-standard path (EOT → ACK)
- Ensure batch file transfers work correctly with slow embedded receivers

**Non-Goals:**
- YModem-g (streaming/no-ACK variant) — lrzsz supports this but it's rarely used; out of scope
- ASCII-mode transfer (CPMEOF stripping in text mode) — binary mode only, matching lrzsz default
- `-f` (full pathname) flag — always send basename, matching lrzsz default
- ZModem protocol changes (separate module, not affected by this change)
- Resume support (YModem has no standard resume mechanism)

## Decisions

### 1. Block 0 metadata: space-separated after single null

**Choice:** Use lrzsz format: `filename\0size mtime mode 0 filesleft totalleft` with sector-count bytes at positions 126-127.

**Rationale:** lrzsz `procheader()` (`lrz.c:1092`) parses block 0 by finding the first null byte for the filename, then using `sscanf` on the remainder for space-separated numeric fields. The current null-separated format (`filename\0size\0mtime\0mode\0`) is incompatible with any receiver expecting lrzsz format. The sector-count trailer (`txbuf[126]`, `txbuf[127]`) enables IMP/KMD bootloader compatibility without breaking lrzsz receivers.

**Alternatives considered:**
- Multi-null format (current): incompatible with lrzsz receivers. Rejected.
- Binary encoding: not standard; most YModem implementations use text-based block 0. Rejected.

### 2. CRC: zero-padded transmit + feed-through verification

**Choice:** Sender appends two zero bytes to CRC computation before transmitting: `crc16_ccitt_zero_pad(data)` (equivalent to `updcrc(0, updcrc(0, crc16_ccitt(data)))`). Receiver feeds received CRC bytes through the CRC engine and checks result is zero.

**Rationale:** This matches lrzsz `wcputsec()` (`lsz.c:1396-1399`) and `wcgetsec()` (`lrz.c:954-963`). The `updcrc(0, updcrc(0, crc))` operation computes `CRC(data || [0, 0])` and the feed-through verification exploits the CRC property that `CRC(data || CRC(data)) == 0`. This is the standard XMODEM CRC method and ensures compatibility with ALL XMODEM/YMODEM implementations.

**Implementation:** Add `crc16_ccitt_zero_pad(data: &[u8]) -> u16` to `crc.rs`, and `crc16_ccitt_verify(data: &[u8], crc_hi: u8, crc_lo: u8) -> bool` for feed-through verification.

**Alternatives considered:**
- Direct-comparison method (current): Sender transmits `crc16_ccitt(data)` directly. While internally consistent, it produces different wire bytes than lrzsz and fails compatibility with receivers that use feed-through verification. Rejected.

### 3. EOT: single-path lrzsz standard handshake

**Choice:** EOT → wait for ACK → done. No post-ACK 'C' probe, no NAK→EOT→ACK variant, no 'C'-as-ACK fallback.

**Rationale:** The current `send_eot()` has four response paths (ACK, ACK+'C', NAK→EOT→ACK, C-as-ACK) that adds complexity and risks consuming the 'C' byte intended for block 0 of the next file. lrzsz `wctx()` (`lsz.c:1351-1364`) simply sends EOT and retries until ACK. The 'C' for the next file is handled by the receiver independently; the sender should not consume it.

**Risk:** Some embedded bootloaders (RT-Thread, U-Boot) may NAK the first EOT before ACKing the second. This is handled at a higher level by the per-file retry loop — if EOT fails after MAX_RETRIES, the file is marked failed and the batch continues.

### 4. CAN: two-byte detection + 10+8 transmit

**Choice:** Sender and receiver detect two consecutive CAN bytes (`CAN, CAN`) before treating them as cancel. The `send_cancel()` function transmits 10 CAN bytes followed by 8 backspace characters (matching lrzsz `canit()`).

**Rationale:** lrzsz `wcgetsec()` (`lrz.c:984-991`) tracks `Lastrx==CAN` and only triggers cancel on two consecutive CAN bytes. A single CAN byte in a data stream can occur from line noise. The 10-CAN+8-BS transmission (`canit.c:35-45`) ensures receivers with different CAN detection thresholds all register the cancel.

### 5. Batch-end: no ACK-wait on empty block 0

**Choice:** After sending the empty block 0 (batch terminator), do not wait for ACK. Send and return immediately.

**Rationale:** lrzsz `wcsend()` (`lsz.c:976-991`) sends the empty block 0 via `wctxpn()` which sends the block but does not wait for ACK. The receiver (`wcreceive()`, `lrz.c:697-698`) ACKs and returns OK. Waiting for ACK after the terminal block is unnecessary and risks a hang if the receiver has already closed its receive loop.

### 6. Inter-file synchronization

**Choice:** After each file completes (EOT ACK received), flush RX buffer to clear stale bytes, then wait for 'C' from receiver before sending next file's block 0. Timeout 10 seconds, retry up to 3 times.

**Rationale:** Slow embedded receivers may still be writing to flash or closing the file when the sender is ready for the next file. The 'C' probe confirms the receiver is alive and ready before sending block 0. If no 'C' after timeout, proceed anyway (optimistic) to handle receivers that don't send an explicit 'C' between files.

## Risks / Trade-offs

- **EOT simplification may break RT-Thread/U-Boot receivers** → Mitigation: The per-file retry loop (MAX_RETRIES=10) already handles EOT retries. If a receiver NAKs EOT, the sender retries. If all retries exhausted, file is marked failed and batch continues. This is acceptable behavior — a failed file is better than a hung transfer.

- **CRC change breaks compatibility with current TauTerm receiver** → Mitigation: Both sender and receiver are updated in the same change. They remain mutually compatible because both use the same new lrzsz-standard method. No version-skew concern.

- **Two-CAN detection may delay cancel by one byte read cycle** → Mitigation: Each byte read has a 1-second timeout. In the worst case, cancel is detected 1 second later than before. The user-facing cancel still responds within a reasonable time.

- **Block 0 format change may not be parseable by non-lrzsz receivers** → Mitigation: lrzsz is the de facto standard. YModem has no formal RFC; all widely-used implementations (Minicom, Tera Term, ExtraPuTTY, embedded bootloaders) follow the lrzsz format. If needed, a format auto-detection could be added later, but there is no known receiver that uses the current null-separated format.

## Open Questions

- **Should we add a compile-time feature flag for "legacy CRC" mode?** → Decision deferred. Not needed unless users report specific devices that require non-lrzsz CRC. Can be added as a follow-up.

- **Should the EOT handshake retain the NAK→EOT→ACK variant behind a configuration option?** → Decision: No. Keep it simple. If specific devices need it, they can be handled via a protocol variant or configuration option added later based on user reports.
