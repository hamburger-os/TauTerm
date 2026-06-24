## 1. Backend: Progress Struct & Event Extension

- [x] 1.1 Extend `TransferProgress` struct in `src-tauri/src/transfer/ymodem.rs` with `file_index: u32`, `total_files: u32`, `aggregate_bytes_transferred: u64`, `aggregate_total_bytes: u64` fields
- [x] 1.2 Update sender `send()` loop to track aggregate counters and populate extended fields on each `on_progress()` call
- [x] 1.3 Update receiver `receive()` loop to track aggregate counters and populate extended fields
- [x] 1.4 Add `transfer-file-start` event emission in sender: emit before `send_block(0, ...)` for each file with payload `{ file_name, file_index, total_files, file_size }`
- [x] 1.5 Add `transfer-file-complete` event emission in sender: emit after successful EOT+ACK for each file with payload `{ file_name, file_index, total_files, bytes_transferred, success: true }`
- [x] 1.6 Add `transfer-file-start` event emission in receiver: emit when receiving a non-empty block 0 (new file metadata parsed)
- [x] 1.7 Add `transfer-file-complete` event emission in receiver: emit when receiving EOT and closing current file
- [x] 1.8 Update `serial.rs` `ymodem_send()` and `ymodem_receive()` progress callbacks to pass extended fields through to `transfer-progress` JSON event payload

## 2. Backend: Per-File Error Recovery in Sender

- [x] 2.1 Define `FileError` struct with `file_name: String`, `error: String` fields in `ymodem.rs` (replaced by inline `BatchFileResult` approach)
- [x] 2.2 Refactor sender `for` loop: change block-0 and EOT `?` to `match { Err(e) => { ... continue; } }`
- [x] 2.3 Accumulate per-file errors into `Vec<BatchFileResult>` across the batch loop
- [x] 2.4 After loop, if errors exist but some files succeeded, return partial-success with `BatchFileResult` per file
- [x] 2.5 After loop, if all files failed, return `Vec<BatchFileResult>` with all `status: "failed"`
- [x] 2.6 Emit `transfer-file-complete` with `success: false` and error detail when a file fails
- [x] 2.7 Update `transfer-complete` event payload to include `files_completed`, `files_failed`, and `results: Vec<{ file_name, status, size, error? }>`
- [x] 2.8 Ensure batch-end empty block 0 is still sent after partial failures

## 3. Backend: CAN Byte Transmission on Cancel

- [x] 3.1 In sender cancel path: before returning error, write `[CAN, CAN]` (0x18 0x18) bytes to port with best-effort (log warning on failure)
- [x] 3.2 In receiver cancel path: before returning error, write `[CAN, CAN]` to port with best-effort
- [x] 3.3 Add short flush after CAN transmission before returning port (100ms sleep + flush call)

## 4. Backend: Commands & Session Wiring

- [x] 4.1 Update `ymodem_send()` in `serial.rs` to collect and return batch result with error details
- [x] 4.2 Update `ymodem_receive()` in `serial.rs` similarly for batch result reporting
- [x] 4.3 `transfer-complete` event reflects batch partial-success semantics with `results` array

## 5. Frontend: Type Definitions

- [x] 5.1 Extend `TransferProgress` interface in `src/types/transfer.ts` with optional `file_index`, `total_files`, `aggregate_bytes_transferred`, `aggregate_total_bytes` fields
- [x] 5.2 Add `FileTransferState` type: `{ fileName, status: 'pending'|'transferring'|'completed'|'failed', bytesTransferred, totalBytes, error? }`
- [x] 5.3 Add `BatchFileResult` interface: `{ file_name, status, size, error? }`
- [x] 5.4 Extend `TransferCompleteEvent` type with optional `files_completed`, `files_failed`, `results: BatchFileResult[]`

## 6. Frontend: TransferContext State Management

- [x] 6.1 Add `batchFiles: Record<string, BatchFileEntry>` to `TransferState` interface
- [x] 6.2 Add reducer action types: `INIT_BATCH`, `FILE_START`, `FILE_COMPLETE`, `RESET_BATCH` (UPDATE_BATCH_FILE merged into SET_PROGRESS)
- [x] 6.3 Implement `INIT_BATCH` reducer: seed `batchFiles` from selected file paths with `pending` status
- [x] 6.4 Implement progress→batch sync: `SET_PROGRESS` reducer updates current file's bytes/status
- [x] 6.5 Implement `FILE_START` reducer: transition file to `transferring`
- [x] 6.6 Implement `FILE_COMPLETE` reducer: transition file to `completed` or `failed`
- [x] 6.7 Add event listeners for `transfer-file-start` and `transfer-file-complete` Tauri events
- [x] 6.8 Update `transfer-complete` event listener to process `results` array and create per-file history entries
- [x] 6.9 Update `sendFiles()` to dispatch `INIT_BATCH` with selected file paths before invoking backend command
- [x] 6.10 Expose `batchFiles` via context value for UI consumption

## 7. Frontend: Multi-File Selection in Dialog

- [x] 7.1 Change `open({ multiple: false })` to `open({ multiple: true })` in `App.tsx` `handleSendFiles`
- [x] 7.2 Update `handleSendFiles` to handle multiple selected paths correctly (already passes array to `transferSend`)

## 8. Frontend: Drag-and-Drop Wired to Transfer

- [x] 8.1 Add Tauri `onDragDropEvent` listener from `@tauri-apps/api/webviewWindow` in `App.tsx`
- [x] 8.2 In drop handler: extract `payload.paths` (file paths array) and call `transferSend(sessionId, paths)`
- [x] 8.3 Keep DOM-level drag-enter/leave/over for visual overlay; Tauri native event handles actual drop
- [x] 8.4 Handle edge case: if no active session, show toast warning instead of silently ignoring

## 9. Frontend: Batch Progress UI in FileTransferPanel

- [x] 9.1 Add conditional rendering: when `batchEntries.length > 0`, show batch UI; otherwise show single-file view
- [x] 9.2 Implement aggregate progress bar: shows total `aggregate_bytes_transferred / aggregate_total_bytes` with "File X of Y" label
- [x] 9.3 Implement scrollable per-file list with status icons (⏳ pending, ⬆️ transferring, ✅ completed, ❌ failed)
- [x] 9.4 Show individual progress bar per file in the list (when transferring, with gradient fill)
- [x] 9.5 Post-batch: retain file list with final status indicators; show error summary for failed files
- [x] 9.6 Update CSS module (`FileTransferPanel.module.css`) with batch layout styles (file list, aggregate bar, status icons)

## 10. Frontend: i18n Strings

- [x] 10.1 Add new translation keys to `zh-CN.json`: `transfer.batchTitle`, `transfer.fileXOfY`, `transfer.filesFailed`, `transfer.partialSuccess`
- [x] 10.2 Add corresponding English translations to `en-US.json`
- [x] 10.3 Update TypeScript i18n interface in `src/i18n/types.ts` with new keys (also added missing `dropHere`, `transferringBanner`, `transferringStatus`)

## 11. Verification

- [x] 11.1 `cargo build` succeeds with all Rust changes (zero warnings)
- [x] 11.2 `tsc --noEmit` succeeds with all TypeScript changes (zero errors)
- [ ] 11.3 Manual test: select 3+ files, verify per-file progress UI, aggregate bar, file list status updates
- [ ] 11.4 Manual test: drag-drop files onto window, verify transfer initiates
- [ ] 11.5 Manual test: cancel mid-batch, verify CAN bytes sent and port returns cleanly
- [ ] 11.6 Manual test: single-file transfer regression (backward compatibility)
- [ ] 11.7 Manual test: receiver batch mode (remote sends 2+ files via YModem)
