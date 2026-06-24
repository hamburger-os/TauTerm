## Context

TauTerm is a Tauri v2 desktop serial terminal application. It implements YModem file transfer over serial connections. The Rust backend in [ymodem.rs](src-tauri/src/transfer/ymodem.rs) contains a full YModem state machine — sender (`YModemSender`) and receiver (`YModemReceiver`) — with CRC-16/CCITT validation, 1024-byte data blocks, block 0 metadata, EOT signaling, and batch-end empty block 0. The backend already iterates over `file_paths: &[String]` in a loop, meaning the send path is structurally ready for batch.

The frontend is built with React + TypeScript. File transfer state is managed via `TransferContext` (useReducer), displayed in `FileTransferPanel`, and triggered from `App.tsx` via file/directory dialogs. A port handoff mechanism in `SessionManager` temporarily transfers the serial port from the I/O thread to the transfer command handler, then returns it.

Current limitations making batch transfer non-functional:
- File dialog uses `multiple: false` (single file only)
- Drag-and-drop has visual feedback but the drop handler is a no-op
- Progress struct (`TransferProgress`) has no batch fields (`file_index`, `total_files`)
- No file-boundary events — frontend must guess file transitions
- Sender loop uses `?` (early return) — one file fails → entire batch aborts
- Cancel doesn't send CAN bytes to remote device
- History concatenates all filenames into one record

## Goals / Non-Goals

**Goals:**
1. Enable multi-file selection in the file dialog (`multiple: true`)
2. Wire drag-and-drop to actually initiate YModem transfer
3. Provide per-file + aggregate progress tracking with file-boundary events
4. Implement per-file error recovery: skip failed files, continue batch, report failures
5. Send CAN bytes on cancellation to notify remote device
6. Create per-file history entries with individual status
7. Extend `TransferProgress` with batch index/count fields

**Non-Goals:**
- Protocol-level changes to YModem (staying within standard YModem spec)
- ZModem or Kermit protocol support
- Transfer speed (bytes/sec) calculation (nice-to-have, deferred)
- Per-file user retry/skip dialog during transfer (simplifies UX — batch continues automatically on error)
- Transfer resume after interruption
- Batch send confirmation dialog (implicit: selecting multiple files = accept batch)

## Decisions

### D1: Progress event extension (additive, backward-compatible)

**Choice**: Add optional fields to the existing `transfer-progress` event JSON payload: `file_index`, `total_files`, `aggregate_bytes_transferred`, `aggregate_total_bytes`. Add two new events: `transfer-file-start` and `transfer-file-complete`.

**Rationale**: The existing event is already emitted per-block. Adding optional fields means existing listeners continue to work. File-boundary events give the frontend explicit signals for per-file UI state transitions without heuristics (guessing based on `file_name` changes).

**Alternatives considered**:
- New event type entirely (e.g., `batch-progress`): More disruptive, duplicate event paths.
- Only use file-name-change heuristic: Fragile — same filename could appear twice in a batch, and empty-block-0 end-of-batch can't be detected from filenames.

### D2: Per-file error recovery — skip-on-failure, collect errors

**Choice**: Change the sender loop from `?` (early return) to `match { Err(e) => errors.push(FileError { ... }); continue; }`. After the loop, if `errors` is non-empty, return all errors as a structured error but deem the operation partially successful (files that had no errors were transferred). The complete event includes a list of failed files.

**Rationale**: In embedded workflows, flashing 3 of 4 files successfully is far better than 0 of 4. The user can retry just the failed files.

**Alternatives considered**:
- Abort-all on first error (current behavior): Drastic — one corrupt file kills the whole batch.
- User prompt per error: Requires bidirectional UI<->backend communication mid-transfer, significant complexity for marginal UX gain. Auto-continue is simpler and addresses the core use case.

### D3: CAN byte transmission on cancel — best-effort

**Choice**: Before the cancel code path returns, write `[CAN, CAN]` (0x18 0x18) to the port. Then flush briefly (100ms drain). If the write fails (e.g., port already closed), log a warning and continue with port return.

**Rationale**: YModem spec requires the cancelling party to send CAN CAN. Not sending it leaves the remote device's receiver in a hanging state, requiring a manual reset. Best-effort is appropriate because port errors at cancel time are expected (the port may already be in an error state).

**Alternatives considered**:
- Don't send CAN (current): Leaves remote device hanging.
- Guaranteed CAN delivery with retries: Over-engineering — if the port is gone, no amount of retrying helps.

### D4: Frontend state — extend TransferState with batch info

**Choice**: Add `batchFiles: Map<string, PerFileState>` to `TransferState`. `PerFileState` tracks: `fileName`, `status` (pending/transferring/completed/failed), `bytesTransferred`, `totalBytes`, `error`. The progress event handler updates both the current-file entry in the map and the aggregate display. File-boundary events transition statuses.

**Rationale**: A map indexed by filename provides O(1) lookup for status updates. The `pending` entries are seeded when the user selects files (from the dialog result or drag-drop paths).

**Alternatives considered**:
- Just use history array + filter: History entries are created post-completion; can't track pending/in-progress states.
- Separate array of `BatchFile` in state: Simpler than a map but O(n) updates.

### D5: Drag-and-drop path extraction

**Choice**: Use Tauri v2's `@tauri-apps/api/webviewWindow` `getCurrentWebviewWindow().onDragDropEvent()` to listen for file drop events that provide native file paths. Replace the current DOM-level drag event handlers (which cannot access file paths in a webview) with Tauri's native drag-drop listener.

**Rationale**: In a Tauri webview, the browser `DragEvent.dataTransfer.files` only provides file names and content, not absolute paths needed for `send_files_ymodem`. Tauri's native drag-drop event provides `payload.paths: string[]`.

**Alternatives considered**:
- Read file content from `dataTransfer.files`, write to temp dir, send temp paths: Overly complex, loses original path context.
- Use `@tauri-apps/plugin-dialog`'s `open()` in drop handler: Misses the point of drag-drop UX.

### D6: History — per-file entries

**Choice**: Each file in a batch generates its own `TransferHistoryItem`. On batch completion, the frontend listener creates one history entry per file with its individual status (completed/failed). The `transfer-complete` event payload is extended with `results: [{file_name, status, size, error?}]`.

**Rationale**: Per-file history allows users to see exactly which files succeeded and which failed, rather than one concatenated "file1, file2, file3" entry that obscures individual outcomes.

## Risks / Trade-offs

- **[R1] Drag-drop may not work on all platforms**: Tauri v2 `onDragDropEvent` is well-supported on Windows/macOS/Linux. → Mitigation: Fall back to file dialog if drag-drop is unavailable.
- **[R2] Large batches (100+ files) may delay progress updates**: Each file triggers at least 2 events (start + complete) plus per-block progress. → Mitigation: Events are lightweight JSON payloads; Tauri's event system handles this volume. No throttling needed for typical embedded batch sizes (2–20 files).
- **[R3] Per-file error recovery may mask systemic issues**: If every file fails (e.g., CRC mismatch due to line noise), the batch still runs to completion, wasting time. → Mitigation: The frontend displays accumulating errors; the user can cancel at any time. A future enhancement could add "abort after N consecutive failures."
- **[R4] Receiver-side batch already works**: The receiver already handles block 0 → new file, EOT → close file, empty block 0 → end batch. No receiver-side changes needed for batch support. Risk is low.
- **[R5] Empty block 0 at batch end**: The current sender already sends this and degrades failure to a warning. This behavior is kept as-is — some embedded YModem implementations exit after EOT+ACK and don't respond to empty block 0.
