# ymodem-batch-error-recovery

## Purpose

Delta spec: 将批量传输的逐文件错误恢复行为从 YModem 扩展到 ZMODEM（XMODEM 仅支持单文件，不适用），同时适配新的 `TransferProtocol` trait 架构。

## MODIFIED Requirements

### Requirement: Per-file error recovery during batch send
The sender SHALL continue to the next file when a single file fails after exhausting retries, rather than aborting the entire batch. Failed files SHALL be reported individually with their error reason. This behavior SHALL apply to both YMODEM and ZMODEM batch transfers.

#### Scenario: Skip failed file and continue batch (YMODEM)
- **WHEN** a file in a multi-file YMODEM batch send fails (e.g., block 0 transmission exhausts all retries, or the file cannot be read)
- **THEN** the sender MUST log the failure with filename and error reason, skip to the next file in the batch, and continue the transfer

#### Scenario: Skip failed file and continue batch (ZMODEM)
- **WHEN** a file in a multi-file ZMODEM batch send fails (e.g., ZFILE frame is rejected, or ZDATA transfer exhausts retries)
- **THEN** the sender MUST log the failure with filename and error reason, skip to the next file in the batch by sending the next ZFILE frame, and continue the transfer

#### Scenario: Batch completion with partial failures
- **WHEN** a batch send completes with some files failed and some succeeded (regardless of protocol)
- **THEN** the `transfer-complete` event payload MUST include `success: true` (the protocol session completed), `files_completed: N`, `files_failed: M`, and a `results` array with `{ file_name, status: "completed"|"failed", size, error? }` for each file

#### Scenario: All files fail
- **WHEN** every file in a batch send fails
- **THEN** the sender MUST return an error indicating all files failed, with individual error details for each file; the `transfer-complete` event payload MUST have `success: false` and `files_completed: 0`

#### Scenario: Batch end signal sent after partial failures
- **WHEN** a batch send has partial failures and reaches the end of the file list
- **THEN** the sender MUST still send the appropriate batch-end signal (empty block 0 for YMODEM, ZFIN for ZMODEM) to properly terminate the session, regardless of prior file failures

#### Scenario: Cancel still aborts entire batch
- **WHEN** the user cancels a batch transfer mid-progress
- **THEN** the sender MUST immediately abort (not skip to next file), returning a cancellation error

### Requirement: Per-file error reporting in frontend
The frontend SHALL display per-file failure information after a batch transfer with errors, and SHALL create individual history entries for each file in the batch, regardless of protocol type.

#### Scenario: Per-file history entries on batch completion
- **WHEN** a batch transfer completes (via `transfer-complete` event with `results` array)
- **THEN** the frontend MUST create one `TransferHistoryItem` per file, each with its own `file_name`, `status` (completed or failed), `size`, and `error` (if failed)

#### Scenario: Error display for failed files
- **WHEN** a batch transfer completes with some failed files
- **THEN** the transfer panel SHALL display an error summary showing which files failed and their error reasons, in addition to marking failed files in the file list with ❌ indicator

#### Scenario: Successful files not marked as errors
- **WHEN** a batch transfer completes with mixed success/failure
- **THEN** successfully transferred files SHALL have `status: "completed"` in their history entries and SHALL NOT trigger error display

## ADDED Requirements

### Requirement: Protocol-agnostic error recovery
Error recovery logic SHALL be implemented in the shared `TransferProtocol` trait layer or individual protocol implementations, not duplicated in `commands.rs`.

#### Scenario: Protocol implementations handle own error recovery
- **WHEN** a protocol-specific error occurs during batch transfer
- **THEN** the protocol implementation (XModem/YModem/ZModem struct) SHALL handle the error according to its protocol specification, and return results via the standard `Vec<BatchFileResult>` format

#### Scenario: Commands layer delegates to protocol
- **WHEN** `commands.rs` invokes `protocol.send_files(...)` or `protocol.receive_files(...)`
- **THEN** the commands layer SHALL NOT contain protocol-specific error recovery logic; it SHALL only handle the returned `Vec<BatchFileResult>` and emit appropriate events
