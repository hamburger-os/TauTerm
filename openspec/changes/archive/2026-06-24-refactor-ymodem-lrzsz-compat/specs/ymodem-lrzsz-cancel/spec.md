## ADDED Requirements

### Requirement: Two-CAN cancel detection
Both YModem sender and receiver SHALL detect cancellation only when two consecutive CAN bytes (0x18) are received, matching the lrzsz `wcgetsec()` two-CAN detection logic.

#### Scenario: Sender detects two consecutive CAN bytes
- **WHEN** the YModem sender receives byte 0x18 followed immediately by another byte 0x18 (no other bytes in between)
- **THEN** the sender SHALL treat this as receiver cancellation and abort the transfer

#### Scenario: Single CAN byte does not trigger cancel
- **WHEN** the YModem sender receives a single byte 0x18 followed by a non-CAN byte (e.g., 0x06 ACK)
- **THEN** the sender SHALL NOT treat this as cancellation and SHALL continue normal processing

#### Scenario: Receiver detects two consecutive CAN bytes
- **WHEN** the YModem receiver receives byte 0x18 followed immediately by another byte 0x18
- **THEN** the receiver SHALL treat this as sender cancellation and abort the transfer

#### Scenario: CAN separated by other bytes does not trigger cancel
- **WHEN** the YModem sender receives 0x18, then 0x06, then 0x18
- **THEN** the sender SHALL NOT treat this as cancellation (the CAN bytes are not consecutive)

### Requirement: lrzsz-standard CAN cancel transmission
The `send_cancel()` function SHALL transmit 10 CAN bytes (0x18) followed by 8 backspace characters (0x08), matching the lrzsz `canit()` cancel sequence.

#### Scenario: Cancel sequence includes 10 CAN + 8 BS
- **WHEN** `send_cancel()` is called to abort a transfer
- **THEN** exactly 10 bytes of 0x18 (CAN) SHALL be written to the serial port, followed by exactly 8 bytes of 0x08 (backspace)

#### Scenario: Cancel sequence flushes port
- **WHEN** `send_cancel()` has written the cancel sequence
- **THEN** `port.flush()` SHALL be called to ensure bytes are transmitted before returning
