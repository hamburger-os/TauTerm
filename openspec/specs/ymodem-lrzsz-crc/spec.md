# ymodem-lrzsz-crc

## Purpose

Defines lrzsz-compatible CRC-16/CCITT computation for YMODEM transfers, including zero-padded CRC transmission and feed-through CRC verification.

## Requirements

### Requirement: CRC-16 zero-padded transmit
The YModem sender SHALL compute the CRC-16/CCITT over data bytes, then feed two zero bytes through the CRC engine (`updcrc(0, updcrc(0, crc))`) before transmitting the resulting CRC as two bytes (high byte first, low byte second). This SHALL match the lrzsz `wcputsec()` CRC transmission method.

#### Scenario: CRC zero-padding produces different wire bytes
- **WHEN** the sender computes CRC for a data block
- **THEN** the transmitted CRC bytes SHALL equal `CRC16(data || [0x00, 0x00])`, which differs from `CRC16(data)` by two additional zero-byte feed-through steps

#### Scenario: CRC bytes are high-byte-first
- **WHEN** the sender transmits the CRC for a block
- **THEN** the first CRC byte SHALL be `(crc >> 8) & 0xFF` and the second CRC byte SHALL be `crc & 0xFF`

### Requirement: CRC-16 feed-through verification
The YModem receiver SHALL verify CRC by feeding the received data bytes through the CRC-16/CCITT engine, then feeding the two received CRC bytes, and checking that the final CRC value equals zero. This SHALL match the lrzsz `wcgetsec()` CRC verification method.

#### Scenario: Feed-through verification succeeds for valid data
- **WHEN** the receiver processes a block where the CRC bytes were computed correctly (including zero-padding on the sender side)
- **THEN** after feeding all data bytes followed by the two received CRC bytes through the CRC engine, the result SHALL be `0x0000`

#### Scenario: Feed-through verification fails for corrupted data
- **WHEN** the receiver processes a block where data bytes were corrupted during transmission
- **THEN** after feeding all received data bytes followed by the two received CRC bytes through the CRC engine, the result SHALL be non-zero, and the receiver SHALL send NAK

#### Scenario: Feed-through verification fails for incorrect CRC bytes
- **WHEN** the receiver processes a block where CRC bytes (but not data) were corrupted
- **THEN** after feed-through verification, the result SHALL be non-zero, and the receiver SHALL send NAK

### Requirement: CRC-16 function parity with lrzsz crctab
The `crc16_ccitt` function SHALL produce identical results to lrzsz's `updcrc` chain for any given byte sequence, using the same 256-entry lookup table with polynomial 0x1021.

#### Scenario: Known vector matches lrzsz
- **WHEN** CRC-16/CCITT is computed on the byte sequence "123456789" (0x31, 0x32, ..., 0x39)
- **THEN** the result SHALL be `0x31C3`, matching the lrzsz test vector
