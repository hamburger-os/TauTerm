## Why

The current `BottomInfoPanel` displays only 4 data points in a single centered row, leaving significant vertical space unused in its resizable panel. Users cannot see protocol-level parameters (baud rate, parity, etc.) or runtime statistics (TX/RX bytes, connection uptime) without digging into configuration dialogs, slowing down embedded development workflows that require constant awareness of connection state.

## What Changes

- Redesign `BottomInfoPanel` layout from single centered row to a left-right split T-style layout
- **Left column (36%)**: session identity — name, connection type, endpoint, status with icons
- **Right column (64%)**: protocol parameters (top sub-area) + runtime statistics (bottom sub-area)
- Introduce protocol-agnostic `SessionProfile` system: each connection type (serial, SSH, Telnet, TFTP) provides its own parameter display via a resolver function
- Add real-time I/O statistics (TX bytes, RX bytes, connection uptime) with 1-second refresh from backend
- Add `StatsCollector` in Rust backend, integrated into session I/O loops via a `StatsReporter` trait
- Add `connectedAt` timestamp tracking for frontend uptime calculation
- New i18n keys for serial parameters, stats labels
- Visual polish: status pulse animation for connecting state, monospace font for technical values

## Capabilities

### New Capabilities
- `session-info-bar`: Redesigned session information panel with left-right split layout, protocol-agnostic profile system, and real-time runtime statistics display

### Modified Capabilities
- `bottom-panel-static`: Significant layout and content change to the info panel within the bottom panel — from single-row to multi-region split layout
- `session-manager`: New requirement for I/O statistics collection (`StatsReporter` trait) and connection timestamp tracking

## Impact

- **Frontend**: `BottomInfoPanel.tsx` + `.module.css` (rewrite), `SessionContext.tsx` (add `SessionStats`, `connectedAt`, `session-stats` event listener), new `profiles/` directory for protocol resolvers
- **Backend**: `session/manager.rs` (add `StatsCollector`), `session/serial.rs` (integrate stats reporting), `commands.rs` (attach `connected_at`), new `StatsReporter` trait
- **i18n**: `en-US.json` and `zh-CN.json` (new labels for serial params, stats)
- **No breaking changes**: existing tab data model extended, not replaced; existing event flow preserved
