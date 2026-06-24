## ADDED Requirements

### Requirement: Block 0 metadata in lrzsz format
The YModem sender SHALL encode Block 0 (file metadata) using the lrzsz-standard format: `filename\0size mtime mode serialno filesleft totalleft`, where numeric fields after the filename's null terminator are space-separated. Additionally, bytes at positions 126 and 127 of the 128-byte block SHALL contain the file length in sectors (little-endian) for IMP/KMD bootloader compatibility.

#### Scenario: Standard block 0 encoding
- **WHEN** the YModem sender constructs block 0 for a file named "test.bin" with size 65536 bytes, mtime 1719619200, and mode 0o100644
- **THEN** the block SHALL contain `test.bin\065536 1719619200 100644 0 1 65536` followed by null-padding to 128 bytes, with block[126] = low byte of (65536+127)>>7 and block[127] = high byte of (65536+127)>>7

#### Scenario: Filename with path stripped
- **WHEN** the YModem sender constructs block 0 for a file with path `/home/user/docs/report.pdf`
- **THEN** the filename in block 0 SHALL be `report.pdf` (basename only, directory stripped)

#### Scenario: Filename exceeds 125 bytes
- **WHEN** the YModem sender constructs block 0 for a file whose basename exceeds 125 characters
- **THEN** the sender SHALL send the block 0 using a 1024-byte STX block instead of a 128-byte SOH block

#### Scenario: Empty block 0 for batch end
- **WHEN** all files in a batch have been sent
- **THEN** the sender SHALL send an empty block 0 (first byte `\0`, remaining 127 bytes zeroed) as a 128-byte SOH block to signal batch termination

#### Scenario: Receiver parses block 0 in lrzsz format
- **WHEN** the YModem receiver receives a non-empty block 0
- **THEN** it SHALL extract the filename from bytes before the first null byte, and parse the size from the space-separated fields following the null byte

#### Scenario: Receiver detects empty block 0
- **WHEN** the YModem receiver receives a block 0 whose first data byte is `\0` (null)
- **THEN** it SHALL treat this as the batch-end signal, ACK the block, and terminate the receive loop

### Requirement: Block 0 sector-count trailer
The YModem sender SHALL write the file's total sector count (128-byte sectors, rounded up) as two bytes at the end of block 0: block0[126] = low byte, block0[127] = high byte.

#### Scenario: Sector count for 1024-byte file
- **WHEN** a file of size 1024 bytes is sent
- **THEN** block0[126] SHALL be `((1024+127)>>7) & 0xFF` = 8 and block0[127] SHALL be `((1024+127)>>7) >> 8` = 0

#### Scenario: Sector count for 128-byte file
- **WHEN** a file of size 128 bytes is sent
- **THEN** block0[126] SHALL be 1 and block0[127] SHALL be 0
