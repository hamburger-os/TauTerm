# ymodem-batch-error-recovery

## Purpose

Defines per-file error recovery behavior during YMODEM batch file transfers, ensuring individual file failures do not abort entire batches, and that the frontend properly reports per-file status.

## Requirements

### Requirement: Per-file error recovery during batch send
The YModem sender SHALL continue to the next file when a single file fails after exhausting retries, rather than aborting the entire batch. Failed files SHALL be reported individually with their error reason.

#### Scenario: Skip failed file and continue batch
- **WHEN** a file in a multi-file YModem batch send fails (e.g., block 0 transmission exhausts all retries, or the file cannot be read)
- **THEN** the sender MUST log the failure with filename and error reason, skip to the next file in the batch, and continue the transfer

#### Scenario: Batch completion with partial failures
- **WHEN** a batch send completes with some files failed and some succeeded
- **THEN** the `transfer-complete` event payload MUST include `success: true` (the protocol session completed), `files_completed: N`, `files_failed: M`, and a `results` array with `{ file_name, status: "completed"|"failed", size, error? }` for each file

#### Scenario: All files fail
- **WHEN** every file in a batch send fails
- **THEN** the sender MUST return an error indicating all files failed, with individual error details for each file; the `transfer-complete` event payload MUST have `success: false` and `files_completed: 0`

#### Scenario: Batch end signal sent after partial failures
- **WHEN** a batch send has partial failures and reaches the end of the file list
- **THEN** the sender MUST still send the batch-end empty block 0 to properly terminate the YModem session, regardless of prior file failures

#### Scenario: Cancel still aborts entire batch
- **WHEN** the user cancels a batch transfer mid-progress
- **THEN** the sender MUST immediately abort (not skip to next file), returning a cancellation error

### Requirement: Per-file error reporting in frontend
The frontend SHALL display per-file failure information after a batch transfer with errors, and SHALL create individual history entries for each file in the batch.

#### Scenario: Per-file history entries on batch completion
- **WHEN** a batch transfer completes (via `transfer-complete` event with `results` array)
- **THEN** the frontend MUST create one `TransferHistoryItem` per file, each with its own `file_name`, `status` (completed or failed), `size`, and `error` (if failed)

#### Scenario: Error display for failed files
- **WHEN** a batch transfer completes with some failed files
- **THEN** the transfer panel SHALL display an error summary showing which files failed and their error reasons, in addition to marking failed files in the file list with ❌ indicator

#### Scenario: Successful files not marked as errors
- **WHEN** a batch transfer completes with mixed success/failure
- **THEN** successfully transferred files SHALL have `status: "completed"` in their history entries and SHALL NOT trigger error display
