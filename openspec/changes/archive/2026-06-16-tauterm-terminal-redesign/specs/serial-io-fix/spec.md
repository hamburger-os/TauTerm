# Serial I/O Fix

## ADDED Requirements

### Requirement: Buffered Write Channel
The system SHALL use a buffered channel (`sync_channel(32)`) for communication between the Tauri command layer and the I/O thread.
The system SHALL NOT use an unbuffered rendezvous channel.

#### Scenario: Rapid user input
- **WHEN** user types 20 characters in quick succession (faster than I/O thread tick)
- **THEN** all 20 characters are queued in the buffer and eventually written to the serial port without blocking the UI thread

#### Scenario: Channel buffer full
- **WHEN** the channel buffer reaches capacity (32 messages)
- **THEN** the sender waits for space, preventing unbounded memory growth

### Requirement: Fair Read-Write Scheduling
The system SHALL alternate between reading and writing in the I/O loop.
The system SHALL NOT skip write checks when read data is available.

#### Scenario: Continuous incoming data with pending writes
- **WHEN** the serial device is continuously sending data AND user types Enter
- **THEN** the user's Enter keystroke SHALL be written to the device within 100ms (not starved by incoming reads)

### Requirement: Reduced Tick Interval
The system SHALL use a tick interval of 1ms in the I/O loop instead of 10ms.

#### Scenario: Low-latency response
- **WHEN** user sends a command
- **THEN** the command is processed and written within 5ms under normal conditions

### Requirement: Non-blocking Write Attempts
The system SHALL attempt to drain all pending writes from the channel in each I/O loop iteration.
The system SHALL NOT limit to processing only one write command per iteration.

#### Scenario: Multiple queued writes
- **WHEN** 5 write commands are queued in the buffer
- **THEN** all 5 are processed in a single I/O loop iteration (not requiring 5 separate ticks)
