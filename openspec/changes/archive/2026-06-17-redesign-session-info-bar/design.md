## Context

The current `BottomInfoPanel` renders a flexbox row with 4 info items (session name, connection type, endpoint, status) centered in the available vertical space. Users can resize the bottom panel up to several hundred pixels, but the content remains one sparse row. The component is protocol-unaware — it displays the same flat structure regardless of connection type, and has no access to protocol-specific parameters (baud rate, parity for serial) or runtime statistics.

The project uses React 18 with CSS Modules, xterm.js, framer-motion, and a custom "Liquid Glass v2" design system. The Rust backend uses Tauri v2 with tokio for async I/O and serialport for serial communication. Session state is managed via React Context + useReducer on the frontend, and `SessionManager` (HashMap + background I/O threads) on the backend.

## Goals / Non-Goals

**Goals:**
- Redesign `BottomInfoPanel` layout to utilize available vertical space with a left-right split
- Display protocol-specific parameters (serial config, SSH auth details, etc.) in the info panel
- Display real-time runtime statistics (TX/RX bytes, connection uptime)
- Build a protocol-agnostic profile system so adding new connection types requires minimal panel code changes
- Maintain existing glass-morphism visual style and theme compatibility

**Non-Goals:**
- Changing the bottom panel's resize behavior or visibility
- Adding interactive controls (buttons, inputs) to the info panel — it remains read-only display
- Changing the StatusBar component
- Adding graphical charts or graphs (keep it text-based)
- Modifying the file transfer panel or tab system

## Decisions

### 1. Protocol Profile System (Resolver Pattern)

**Decision:** Use a simple function-based resolver pattern rather than a React component-per-protocol approach.

```typescript
type ProfileResolver = (meta: SessionMeta) => SessionProfile;
```

**Alternatives considered:**
- **Per-protocol React components**: More flexible (custom JSX per type) but over-engineered for what is essentially a list of label-value pairs. Harder to maintain visual consistency.
- **Single giant component with switch/case**: Simplest but messy with 4+ protocol types, violates open/closed principle.

**Rationale:** The resolver returns a pure data structure (`SessionProfile` with `identity` and `parameters` arrays). The panel component renders this data uniformly. Adding a new protocol = one new function + i18n strings. No component changes needed.

### 2. Stats Reporting Architecture

**Decision:** Use a `StatsReporter` trait in Rust with a shared `mpsc::Sender<SessionStats>` channel per session, polled by a central `StatsCollector` in `SessionManager` at 1-second intervals.

```
SerialSession I/O loop ──→ StatsReporter::report() ──→ mpsc channel
                                                           │
SessionManager::StatsCollector (tokio interval 1s) ←──────┘
        │
        ▼
  emit("session-stats", SessionStats) ──→ Frontend SessionContext
```

**Alternatives considered:**
- **Counting in frontend from data events**: Inaccurate (filters, control chars, encoding overhead). The backend has the ground truth.
- **Counting per read/write syscall with AtomicU64**: No additional channel needed, but polling atomics from a different thread is not idiomatic in Rust without `Arc`. Channel approach is cleaner.
- **WebSocket push from I/O thread directly**: Overkill for simple stats; Tauri events are simpler and already used.

**Rationale:** The channel approach decouples stats gathering from stats emission. Each session type implements `StatsReporter` once; the collection logic is shared.

### 3. Frontend Stats Timer

**Decision:** Use `setInterval(1000)` in a `useEffect` to update the uptime display, driven by `connectedAt` timestamp. TX/RX bytes update reactively from Tauri events.

**Alternatives considered:**
- **requestAnimationFrame**: Overkill — 1-second granularity is sufficient for uptime display
- **Backend-driven uptime string**: Adds serialization overhead; frontend can compute display format more flexibly (i18n, format preference)

### 4. i18n Key Strategy

**Decision:** Add new keys under existing namespaces where applicable:
- `serial.baudRate`, `serial.dataBits`, `serial.parity`, `serial.stopBits`, `serial.flowControl` — extend existing `serial.*` namespace
- `stats.txBytes`, `stats.rxBytes`, `stats.uptime` — new `stats.*` namespace
- `session.sessionName`, `session.connectionType`, `session.endpoint`, `session.status` — refine existing keys

## Risks / Trade-offs

- **[1-second stats polling overhead]** → Negligible: the channel poll is non-blocking, no allocation per tick. Stats are tiny structs (~40 bytes). Only sends Tauri event if stats changed.
- **[connectedAt accuracy]** → `connectedAt` captures the moment the Tauri command returns, not when the serial port actually opened (microseconds difference). Acceptable for human-facing display.
- **[Protocol extension surface]** → Adding a new protocol type now requires implementing `StatsReporter` on the Rust side AND creating a `ProfileResolver` on the frontend side. This is intentional — ensures new protocols are fully integrated. Documented as part of the "adding new protocol" guide.
- **[Panel minimum height]** → At 120px minimum, the split layout is comfortable. Below that (~80px), the stats area may clip. Add `min-height: 120px` constraint on the panel container.
