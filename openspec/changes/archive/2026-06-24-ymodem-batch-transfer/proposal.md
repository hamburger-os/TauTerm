## Why

The YModem protocol inherently supports batch file transfer (multiple files in a single session, terminated by an empty block 0), and the Rust backend already implements the full batch loop including proper empty-block-0 batch termination. However, the frontend only allows selecting a single file, drag-and-drop is visual-only (doesn't initiate transfer), progress tracking lacks per-file granularity, and the sender aborts the entire batch on any single-file failure. These gaps prevent users from leveraging batch transfer in real embedded development workflows where flashing multiple firmware binaries, config files, and assets in one session is common.

## What Changes

- **Frontend multi-file selection**: File dialog changed from `multiple: false` to `multiple: true`
- **Drag-and-drop wired to transfer**: Drop events extract file paths and invoke YModem send
- **Multi-file progress tracking**: `TransferProgress` struct extended with `file_index`, `total_files`, and aggregate byte counts; progress events include file-boundary notifications
- **Per-file + aggregate progress UI**: File transfer panel shows per-file status list with aggregate batch progress bar
- **Per-file error recovery**: Sender loop skips failed files (after max retries exhausted) and continues with remaining files instead of aborting the entire batch
- **CAN byte transmission on cancel**: When user cancels, sender transmits `CAN CAN` (0x18 0x18) to notify the remote receiver before returning the port
- **Per-file history entries**: Each file in a batch creates its own transfer history record
- **File boundary events**: Backend emits `file-start` and `file-complete` events so the frontend can track per-file state without heuristics

## Capabilities

### New Capabilities

- `ymodem-batch-progress`: Multi-file progress tracking — per-file status (pending/transferring/completed/failed), aggregate batch progress with `file_index`/`total_files` counters, file-boundary events (`file-start`/`file-complete`), and UI components to render per-file + aggregate progress views.
- `ymodem-batch-error-recovery`: Per-file error handling during batch transfers — when a single file fails after exhausting retries, the sender skips to the next file instead of aborting the entire batch; failed files are reported individually with their error reason.

### Modified Capabilities

<!-- No existing spec requirements are changing. The existing file-transfer spec already defines batch send/receive, drag-drop initiation, and CAN cancellation — these are implementation gaps being filled. -->

## Impact

- **Rust backend** (`src-tauri/src/transfer/ymodem.rs`): Sender loop error recovery, CAN transmission on cancel, progress callback signature extension
- **Rust backend** (`src-tauri/src/commands.rs`): Progress event payload extensions
- **Rust backend** (`src-tauri/src/session/serial.rs`): Progress callback wrapper adaptation
- **Frontend types** (`src/types/transfer.ts`): `TransferProgress` interface extended with batch fields, new `FileTransferStatus` type
- **Frontend context** (`src/context/TransferContext.tsx`): Multi-file progress state management, per-file history, file-boundary event handling
- **Frontend UI** (`src/components/FileTransfer/FileTransferPanel.tsx`): Per-file list + aggregate progress bar, batch summary
- **Frontend App** (`src/App.tsx`): File dialog `multiple: true`, drop handler wired to `transferSend()`
- **i18n** (`src/i18n/locales/`): New translation keys for batch UI strings
