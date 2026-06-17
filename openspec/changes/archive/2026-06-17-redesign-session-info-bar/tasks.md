## 1. Backend — Stats Infrastructure

- [x] 1.1 Define `SessionStats` struct and `StatsReporter` trait in `src-tauri/src/session/mod.rs`
- [x] 1.2 Add stats channels (`mpsc::Sender<SessionStats>`, `mpsc::Receiver<SessionStats>`) to `SessionHandle` in `manager.rs`
- [x] 1.3 Implement `StatsCollector` in `SessionManager` — tokio interval (1s) that polls all stats receivers and emits `session-stats` Tauri events
- [x] 1.4 Integrate `StatsReporter` into `SerialSession` I/O loop — increment TX counter after writes, RX counter after reads
- [x] 1.5 Add `connected_at` timestamp to `SessionHandle`, set on successful connect in `commands.rs`, include in `session-connected` event payload

## 2. Frontend — Data Model & Context

- [x] 2.1 Add `SessionStats` interface (`txBytes`, `rxBytes`) and `connectedAt` field to `TabInfo` in `SessionContext.tsx`
- [x] 2.2 Add `UPDATE_TAB_STATS` reducer action and dispatch logic
- [x] 2.3 Add Tauri event listener for `session-stats` in `SessionProvider` — parse event payload and dispatch `UPDATE_TAB_STATS`
- [x] 2.4 Store `connectedAt` from `session-connected` event payload when creating/updating tabs

## 3. Frontend — Protocol Profile System

- [x] 3.1 Create `src/profiles/types.ts` — define `ProfileItem`, `SessionProfile`, `ProfileResolver` types
- [x] 3.2 Create `src/profiles/serial.ts` — implement `serialProfile` resolver: identity items (name, type, endpoint, status) + parameter items (baud_rate, data_bits, parity, stop_bits, flow_control)
- [x] 3.3 Create `src/profiles/index.ts` — profile registry that maps `ConnectionType` to resolver, with fallback for unknown types

## 4. Frontend — BottomInfoPanel Rewrite

- [x] 4.1 Rewrite `BottomInfoPanel.tsx` — implement left-right split layout using CSS Grid, render identity section, parameters section, and stats section from resolved `SessionProfile`
- [x] 4.2 Create `InfoItemRow` sub-component — renders a single label-value pair with optional icon and monospace styling
- [x] 4.3 Create `StatsSection` sub-component — renders TX bytes, RX bytes (formatted with B/KB/MB), and uptime (HH:MM:SS from `connectedAt`)
- [x] 4.4 Implement uptime timer — `setInterval(1000)` in `useEffect`, driven by `connectedAt` timestamp
- [x] 4.5 Implement byte formatting utility — auto-selects B, KB, or MB with 1 decimal place
- [x] 4.6 Rewrite `BottomInfoPanel.module.css` — grid layout (left `minmax(180px, 36%)`, right `1fr`), glass-border divider, typography (11px labels, 13px values, 14px stats), monospace values
- [x] 4.7 Add connecting state pulse animation in CSS (`@keyframes pulse`) and status color classes
- [x] 4.8 Ensure empty state ("No active session") still renders correctly

## 5. i18n

- [x] 5.1 Add serial parameter i18n keys (`serial.baudRate`, `serial.dataBits`, `serial.parity`, `serial.stopBits`, `serial.flowControl`) to `en-US.json` and `zh-CN.json`
- [x] 5.2 Add stats i18n keys (`stats.txBytes`, `stats.rxBytes`, `stats.uptime`) to both locale files
- [x] 5.3 Add fallback/unavailable status keys if missing

## 6. Integration & Polish

- [ ] 6.1 Verify panel renders correctly at various heights (120px min, 200px default, resized larger)
- [ ] 6.2 Verify stats update in real-time during active data transfer
- [ ] 6.3 Verify uptime timer starts on connect, stops on disconnect, resets on reconnect
- [ ] 6.4 Verify serial parameters display correctly for various configurations (different baud rates, parity options, etc.)
- [x] 6.5 Verify empty state renders when no session is active
- [ ] 6.6 Verify theme compatibility — test with neon-dark, ocean, and sunset themes
