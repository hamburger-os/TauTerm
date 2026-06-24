# io-channel-abstraction

## Purpose

定义协议无关的 I/O 通道抽象层，使 `spawn_io_loop` 引擎不依赖任何具体传输类型，同时支持同步和异步两种 I/O 执行模式。

## ADDED Requirements

### Requirement: Channel trait provides protocol-agnostic I/O interface
The system SHALL define a `Channel` trait with methods: `read(&mut [u8]) -> Result<usize>`, `write(&[u8]) -> Result<usize>`, `flush() -> Result<()>`, `set_timeout(Duration) -> Result<()>`, and `is_connected() -> bool`.
The `Channel` trait SHALL be implemented for `SerialChannel`, `TcpChannel`, `SshChannel`, `PipeChannel`, and any future transport type.
The `Channel` trait SHALL be object-safe (usable as `Box<dyn Channel>`).

#### Scenario: Serial port implements Channel
- **WHEN** a `SerialChannel` wrapping a `serialport::SerialPort` receives a `read()` call
- **THEN** it SHALL delegate to the underlying serial port's `read()` and return the bytes read

#### Scenario: TCP stream implements Channel
- **WHEN** a `TcpChannel` wrapping a `tokio::net::TcpStream` receives a `write()` call
- **THEN** it SHALL write the bytes to the TCP stream and return the count written

### Requirement: I/O loop engine is transport-agnostic
The system SHALL provide a `spawn_io_loop(channel: Box<dyn Channel>, ...)` function that drives the read/write/cancel cycle without any knowledge of the underlying transport type.
The I/O loop SHALL maintain fair read/write scheduling with atomic TX/RX byte counters.
The I/O loop SHALL support port-handoff for Inline transfer strategy.

#### Scenario: I/O loop dispatches data from any channel type
- **WHEN** the I/O loop reads data from a `TcpChannel`
- **THEN** it SHALL emit the data via the `on_data` callback exactly as it does for `SerialChannel`

#### Scenario: I/O loop handles channel disconnection
- **WHEN** `channel.is_connected()` returns false or a read error occurs
- **THEN** the I/O loop SHALL call `on_disconnect` and exit the loop

### Requirement: Dual-mode I/O strategy supports sync and async protocols
The system SHALL support two I/O execution modes: `Sync` (using `std::thread`) for blocking transports, and `Async` (using `tokio::spawn`) for non-blocking transports.
A plugin SHALL declare its I/O strategy in the `ProtocolAdapter` implementation.
The sync mode SHALL use `std::sync::mpsc` for the write command channel.
The async mode SHALL use `tokio::sync::mpsc` for the write command channel.

#### Scenario: Serial plugin uses sync I/O mode
- **WHEN** the Serial plugin creates a session
- **THEN** the kernel SHALL spawn a `std::thread`-based I/O loop with the serial `Channel`

#### Scenario: SSH plugin uses async I/O mode
- **WHEN** the SSH plugin creates a session
- **THEN** the kernel SHALL spawn a `tokio::task`-based I/O loop with the SSH `Channel`

### Requirement: Channel supports transfer port handoff
A `Channel` implementation MAY support port handoff by implementing an optional `try_handoff() -> Option<Box<dyn Any>>` method.
Channels that do not support handoff SHALL return `None`, signaling the Transfer Manager to use SideChannel or SeparateConnection strategy.

#### Scenario: Serial channel supports handoff
- **WHEN** the Transfer Manager calls `channel.try_handoff()` on a `SerialChannel`
- **THEN** it SHALL return `Some(Box<dyn SerialPort>)` for exclusive YModem access

#### Scenario: SSH channel rejects handoff
- **WHEN** the Transfer Manager calls `channel.try_handoff()` on an `SshChannel`
- **THEN** it SHALL return `None`, and the Transfer Manager SHALL use the SideChannel strategy
