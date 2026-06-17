# session-manager (delta)

## Purpose

新增 I/O 统计收集和连接时间戳要求，支持会话信息栏的运行时统计显示。

## ADDED Requirements

### Requirement: I/O Statistics Collection
The system SHALL collect per-session I/O byte counts during read and write operations. Each session's I/O thread SHALL increment TX and RX counters for every successfully transmitted or received byte. The system SHALL expose current statistics via a `StatsReporter` trait that each `SessionImpl` variant implements.

#### Scenario: Serial session transmits data
- **WHEN** a serial session sends 256 bytes of data
- **THEN** the session's TX byte counter SHALL increment by 256

#### Scenario: Serial session receives data
- **WHEN** a serial session reads 1024 bytes from the port
- **THEN** the session's RX byte counter SHALL increment by 1024

#### Scenario: I/O error does not corrupt stats
- **WHEN** a serial read returns an error (e.g., timeout)
- **THEN** the session's RX counter SHALL remain unchanged
- **AND** the session SHALL NOT crash or panic

### Requirement: Stats Event Emission
The system SHALL emit I/O statistics to the frontend at 1-second intervals via the Tauri event `session-stats`. Each event payload SHALL contain the session's tab ID, TX byte count, RX byte count, and connection timestamp.

#### Scenario: Periodic stats emission
- **WHEN** a session is connected and I/O is active
- **THEN** the system SHALL emit a `session-stats` event every 1 second containing the current TX and RX byte counts

#### Scenario: No stats emission for disconnected session
- **WHEN** a session is in "disconnected" state
- **THEN** the system SHALL NOT emit `session-stats` events for that session

#### Scenario: Stats emission stops on session close
- **WHEN** a session is closed
- **THEN** the StatsCollector SHALL be dropped, and no further `session-stats` events SHALL be emitted for that session

### Requirement: Connection Timestamp Tracking
The system SHALL record the Unix timestamp (milliseconds) when a session successfully connects. This timestamp SHALL be included in the `session-connected` event payload and persisted in the session state.

#### Scenario: Session connects successfully
- **WHEN** a serial session successfully opens COM3 and starts its I/O thread
- **THEN** the `session-connected` event payload SHALL include a `connected_at` field with the current Unix timestamp in milliseconds

#### Scenario: Reconnection updates timestamp
- **WHEN** a previously disconnected session reconnects
- **THEN** the `connected_at` timestamp SHALL be updated to the new connection time
