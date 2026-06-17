# ymodem-batch-progress

## Purpose

Defines multi-file progress tracking for YMODEM batch transfers, including per-file status events, aggregate progress display, and file boundary event handling in the frontend.

## Requirements

### Requirement: Multi-file progress tracking in backend
The backend SHALL emit progress events with batch context fields (`file_index`, `total_files`, `aggregate_bytes_transferred`, `aggregate_total_bytes`) and file-boundary events (`transfer-file-start`, `transfer-file-complete`) during multi-file YMODEM transfers.

#### Scenario: Batch progress with file index
- **WHEN** a YMODEM batch send or receive transfers file N of M total files
- **THEN** the `transfer-progress` event payload MUST include `file_index: N` (0-based), `total_files: M`, `aggregate_bytes_transferred` (sum of all completed files + current file progress), and `aggregate_total_bytes` (sum of all file sizes in batch)

#### Scenario: File start boundary event
- **WHEN** the sender begins transmitting block 0 for a file or the receiver receives a non-empty block 0 for a new file
- **THEN** a `transfer-file-start` event MUST be emitted with payload `{ file_name, file_index, total_files, file_size }`

#### Scenario: File complete boundary event
- **WHEN** the sender receives ACK for EOT or the receiver receives EOT for the current file
- **THEN** a `transfer-file-complete` event MUST be emitted with payload `{ file_name, file_index, total_files, bytes_transferred, success: true }`

#### Scenario: Single-file transfer backward compatibility
- **WHEN** a YMODEM transfer involves only one file (`total_files == 1`)
- **THEN** the extended progress fields (`file_index`, `total_files`, `aggregate_*`) SHALL still be present in events, and `file_index` SHALL be 0

### Requirement: Multi-file progress UI
The file transfer panel SHALL display per-file status for batch transfers, including an aggregate progress bar showing total batch progress and a scrollable file list with individual file status indicators.

#### Scenario: Batch file list display
- **WHEN** a batch YMODEM transfer is in progress with multiple files
- **THEN** the transfer panel MUST show a list of all files in the batch, each labeled with its filename, individual progress percentage, and status icon (pending ⏳, transferring ⬆️/⬇️, completed ✅, failed ❌)

#### Scenario: Aggregate progress bar
- **WHEN** a batch YMODEM transfer is in progress
- **THEN** a single aggregate progress bar MUST display total batch progress (total bytes transferred / total bytes across all files), along with a label showing "File X of Y"

#### Scenario: Single-file mode unchanged
- **WHEN** a single-file YMODEM transfer is in progress
- **THEN** the transfer panel MUST display the same single-file progress view as before, without showing a file list

#### Scenario: Post-batch status summary
- **WHEN** a batch transfer completes (some files may have failed)
- **THEN** the file list MUST retain its display with final status indicators, and the aggregate bar MUST show final state

### Requirement: File boundary event handling in frontend
The frontend transfer context SHALL listen for `transfer-file-start` and `transfer-file-complete` events and update per-file tracking state accordingly.

#### Scenario: Seeding batch file list on transfer start
- **WHEN** a batch transfer is initiated with N file paths selected
- **THEN** the frontend state MUST be initialized with N entries in `batchFiles`, each with status `pending`

#### Scenario: Updating file status on start event
- **WHEN** a `transfer-file-start` event is received for file at index I
- **THEN** the `batchFiles[I]` entry MUST transition from `pending` to `transferring`

#### Scenario: Updating file status on complete event
- **WHEN** a `transfer-file-complete` event is received for file at index I
- **THEN** the `batchFiles[I]` entry MUST transition from `transferring` to `completed` (if `success: true`) or `failed` (if `success: false`)

#### Scenario: Aggregate progress updates
- **WHEN** any `transfer-progress` event is received during a batch transfer
- **THEN** the aggregate `bytes_transferred` and `total_bytes` SHALL be updated from the event's `aggregate_*` fields, and the current file's individual progress SHALL be updated from the event's `bytes_transferred`/`total_bytes` fields
