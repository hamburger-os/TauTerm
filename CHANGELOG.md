# Changelog

All notable changes to TauTerm will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Protocols
- **Network Debug (TCP/UDP)** — merges the planned TCP Raw and UDP Monitor into a single network debug session:
  - TCP Client / TCP Server (multi-client with configurable cap); UDP Client = fixed send target on an unconnected socket (sends to a fixed remote, receives from any source incl. broadcast/multicast), UDP Server = single connectionless session (local bind, unicast / broadcast / multicast with IGMP join, TTL, interface, self-receive)
  - TCP uses a container session + per-peer channel model (RFC 4254 Channel Mechanism): each client gets its own I/O loop, stats, CommHandle (encoding / auto-reply / Lua scripts isolated per peer), logging and `session-data` stream; **UDP is peerless** — a single socket `recv_from` emits each datagram as a `session-data` event carrying the source address
  - **Peer navigation (TCP) moved into the left session tree** (non-tab child nodes with status dot); peers are auto-named "Peer N" like SSH's "Channel N" (address shown as the second line); clicking a peer routes to the container view and selects it; clicking the container deselects; right-click offers Disconnect / Remove tombstone
  - Endpoint addresses are uniform across roles with a transport prefix (`tcp://host:port` / `udp://host:port`); disconnected sessions show a blank data area like terminal sessions (state conveyed by the status bar)
  - TCP Client is a plain single session (auto-selects its only peer, disconnects when the peer leaves); UDP Client is a fixed-remote single session with no peer tree and shows its own bound ip:port in the endpoint line
  - **Bare view (serial-like)**: no in-view header or toolbar — identity lives in the sidebar, TX/RX stats in the status bar; **data mode (Dual/Text/Hex) is TCP-only** (connect-time `data_mode` param, no in-session switch); UDP always shows a dual HEX+ASCII per-datagram grid
  - **SendBar moved to the global bottom position**, with a **unified send-target bar (`TargetBar`) shared across all four send modes** (basic / command / auto-reply / script); TCP Server shows a peer dropdown with an "all clients" pseudo-target for broadcast, UDP Server shows a manual-target address input with a recent-source quick-reply dropdown; broadcast / multicast addresses are typed manually (e.g. `255.255.255.255:port`); `SessionContext.sendToTarget` routes the current target for frontend sends
  - **Broadcast-to-all (TCP) is now a send-target value** (the "all clients" option in the target bar) rather than a separate mode-switcher toggle; `sendData` fan-out remains for SSH multi-channel sessions; UDP broadcast/multicast is network-layer via the manual target address; the script engine's `send()`/`send_text()` route to the synced current target, with new `send_to(target, data)` / `send_to_text(target, data)` Lua functions for explicit UDP targets
  - Dual / Text / Hex display for TCP streams (delimiter + timeout framing), per-datagram packet grid for UDP showing the full source/target timeline (datagram boundaries preserved, no per-source filtering)
  - Peer lifecycle hardening (TCP): two-phase `close_sub_connection` (signal under lock, join outside) eliminating a lock-cycle deadlock; peer disconnects settle state + final stats and emit `netdbg-peer-left` with final TX/RX bytes; TCP Server cap counts only connected peers (disconnected tombstones free their slots)
  - `max_clients` configurable in the connect dialog (TCP Server)
  - Manual send supports TEXT/HEX toggle; TX bytes counted into session stats; send failures surfaced via toast
  - Multicast group validated as IPv4 multicast (224.0.0.0–239.255.255.255) with inline hint
  - No file transfer (by design), no auto-reconnect / heartbeat (debugger must observe disconnects)

### UI
- **Sidebar session cards: clearer separation & tighter left alignment** — unselected cards now carry a persistent subtle border (`--glass-border-default`) so long session lists read as distinct items instead of one text block (hover/active states unchanged); left-side whitespace trimmed (expand-arrow placeholder 16px → 10px, list/container padding reduced, text starts ~15px closer to the panel edge); card vertical spacing 2px → 4px; endpoint line font-size tokenized (`--text-xs`); header/search/cards/settings-button left edges unified on one alignment baseline; child-tree indent 16px → 12px
- **New-session dialog: responsive mode grid** — the connection-type card grid switches from a fixed 2 columns to `auto-fit` so it flows to 3 columns (and more as space allows); the dialog width is now adaptive (`min(560px, 100vw - 32px)`); card labels reserve two lines so every card keeps a uniform size when a label wraps

### Fixed
- **Network Debug: background receive lost for inactive sessions** — the `session-data` listener resolved peers from a ref that is only refreshed while the view renders; an inactive (unmounted) session froze that ref to an empty list, so all data arriving in the background was dropped and the data area appeared empty on switch. The listener now resolves peers from `SessionContext.stateRef` (always fresh), so frames accumulate per session even while inactive and are visible immediately on switch.
- **Network Debug: server data area stays empty after tab switching** — the `session-data` listener was registered once per session by the plugin session store, so its closure kept the peer refs from the first mount; after remounting (tab switch) those refs froze at the initial empty list and the server view never matched its peers. The listener now resolves the current refs from a per-render registry, so incoming data is routed to the correct container's frame store again.
- **Network Debug: client shows its own bound address** — peers now carry a `local_addr` (client's OS-assigned socket address after connecting); a TCP/UDP client session card appends it to the endpoint line (`tcp://127.0.0.1:8080 · 127.0.0.1:56780`, single line with ellipsis), letting you map the client to the matching "Peer N" row on the server side without changing the card height.

### Developer Experience
- Remove Claude Code skill shims — `.agents/skills/` is now the sole canonical skill location. Deleted `scripts/gen-skill-shims.mjs`, the `skills:sync` npm script, the `.claude/` directory (including `settings.json`), and the CI drift check.

## [0.4.0] — 2026-07-22 (First Public Tech Preview)

This is the first public release of TauTerm, a cross-platform terminal emulator built with Tauri v2 featuring a microkernel plugin architecture.

### Core Architecture
- 8-module microkernel (window, tab, IPC, config, plugin, theme, shortcut, i18n)
- `ProtocolAdapter` trait for protocol plugins
- `Channel` / `AsyncChannel` trait I/O abstraction layer
- Dual-mode I/O strategy (sync for Serial, async for SSH)
- Plugin Host with lifecycle management (discover → load → initialize → ready → stop → unload)

### Protocols
- **Serial** (RS-232/485) — full support with automatic port enumeration, baud rate configuration, and flow control
- **SSH** — password and key authentication, SideChannel architecture, SFTP file transfer via russh-sftp

### Terminal Engine
- xterm.js-based terminal with multi-instance pool management
- Three data display modes: Text, HEX, and Dual (split-view with TX/RX color coding)
- Terminal search (`Ctrl+F`) with case toggle and result navigation

### File Transfer
- Three-strategy transfer subsystem: Inline (YModem/XModem/ZModem for Serial), SideChannel (SFTP for SSH), SeparateConnection
- Right sidebar panel with per-session protocol configuration
- Unified progress events and cancel signaling

### Virtual Serial Port Bridge (Windows)
- com0com kernel driver integration — auto-creates COM port pairs when connecting via Serial
- Bidirectional I/O bridge between physical and virtual ports
- Orphan port cleanup on startup, admin elevation for driver operations
- NSIS installer with automatic driver install/uninstall hooks

### Sending Bar
- **Basic Send**: Text/HEX mode, newline control, loop sending, command history
- **Command Panel**: Predefined command sequences with drag-to-reorder and loop execution
- **Auto-Reply**: Visual rule configuration, 5 match modes, 10 dynamic macros, timer triggers
- **Script Editor**: Embedded Lua 5.4 runtime (mlua), per-session VM sandbox, code generation
- Background execution — scripts continue running when switching tabs

### Liquid Glass v3 Design System
- Three themes: Google Glow, Obsidian, Frosted (light)
- Animated gradient background with SVG noise texture
- Framer Motion transitions throughout
- Custom title bar, frameless window, glass-morphism panels
- CSS Modules + CSS custom properties for zero-hardcoded-colors

### Developer Tools (Right Sidebar)
- Checksum calculator (CRC8/16/32 with presets)
- Encoding converter (Base64, HEX, float, endianness)
- Bit operations and C sizeof calculator
- Protocol parser (Modbus RTU/ASCII, AT commands)

### i18n
- Chinese (zh-CN) and English (en-US) with i18next namespace isolation
- Plugins bundle their own translations
- Runtime language switching

### Settings
- 7-panel fullscreen overlay: General, Appearance, Language, Encoding, Logging, Shortcuts, About
- Real-time font size and line buffer slider preview
- Customizable keyboard shortcuts with recording mode and conflict detection

### Logging
- System event log (`TauTerm_YYYYMMDD.log`) with auto-rotation
- Session data log with text/hex/dual formatting and expiry cleanup
- Status bar indicator, right-click enable/disable

### Session Management
- Offline session configuration (create/edit without connecting)
- Persistent sessions with reconnect support
- Unified tab bar with drag-to-reorder
- Command palette (`Ctrl+Shift+P`) with fuzzy search

### Credential Store
- In-memory credential management with type-safe API (password/key/certificate/token)
- Full CRUD operations via Tauri commands (`store_credential`, `get_credential`, `list_credentials`, `delete_credential`)
- OS-native keyring and AES-256-GCM encrypted file fallback planned for v0.5

### Security
- Log redaction — auto-filters passwords, private keys, and tokens from log output
- SSH host key verification — first-connect fingerprint confirmation dialog (SHA-256); known_hosts persistent storage planned for v0.5

### CI/CD
- GitHub Actions workflow for Windows (NSIS/MSI), Linux (deb/rpm/AppImage), macOS (dmg/app)
- Linux virtual serial port support via socat backend (SocatBackend implementing VirtualPortBackend trait)
- Platform-conditional build system
