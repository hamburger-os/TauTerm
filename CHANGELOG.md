# Changelog

All notable changes to TauTerm will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Protocols
- **TRDP Node + Monitor** — adds a first-party custom TRDP session backed by TCNOpen 3.0.0.0. Node sessions can combine PD Publisher/Subscriber/Request and MD Notify/Request/Listener-Replier roles, including MD UDP/TCP Reply/ReplyQuery/Confirm flows, A/B link selection, topology/redundancy metadata and diagnostics. PD Request/MD Request/MD Notify use one-shot Send semantics; persistent objects use explicit Start/Stop. Subscriber/PD Request timeout is explicit Auto/Custom rather than a hard-coded cycle multiplier.
- **TRDP XML, Dataset and capture tooling** — imports TCNOpen-style XML, maps ComID to Dataset definitions, decodes/encodes structured payload fields while keeping raw HEX as the wire truth source, opens pcap/pcapng, performs live one/two-interface Monitor capture with MD/TCP stream reassembly and saves pcapng with A/B provenance preserved. Monitor decodes the MD UUID/status/timeout/URI wire header at the correct offsets and validates TRDP header CRC/protocol compatibility. XML import preserves PD/MD telegram classification so MD entries are not auto-created as PD Subscriber templates.
- **TRDP safety boundary** — detects/preserves SDT-related metadata/raw traffic but intentionally does not validate SDTv2/SDTv4 safety semantics or claim safety certification.

### Platforms
- **TRDP native sidecar** — vendors the required TCNOpen TRDP 3.0.0.0 MPL-2.0 source and builds it offline as a Tauri `externalBin` sidecar. Windows live Monitor dynamically loads user-installed Npcap; Linux/macOS use system libpcap; offline capture analysis requires neither.

### Developer Experience
- **TRDP interoperability coverage** — adds a small TCNOpen C reference peer, sample TRDP XML/Workspace files, cross-platform native builds, Linux PD/MD UDP/MD TCP/ReplyQuery→Confirm loopback tests and a Linux `.deb` smoke check that verifies the packaged sidecar.

## [0.6.0] — 2026-09-03

### Workspace
- **Split View (1–4 panes)** — adds nested multi-pane layouts with selected-pane session context, draggable dividers, pane collapse behavior and sidebar placement cues so SSH, Serial, Local Shell and network-debugging sessions can stay visible together.
- **Persistent Workspace context** — persists the last valid pane tree, normalized split ratios, stable saved-session placement and selected pane across app restarts. Restored sessions stay disconnected until the user explicitly connects them; sockets, PTYs, credentials, terminal process state and automatic reconnect are never persisted as Workspace state.
- **Safe restore and direct reconnect workflow** — missing or deleted saved sessions are pruned without collapsing the pane tree, corrupt/stale/future Workspace payloads fall back safely, and disconnected panes expose Connect / Configure / Delete directly. Runtime SSH and Local Shell child channels are canonicalized to stable parent configurations for durable placement.

### Security
- **Credential storage trust closure** — adds cross-platform persistence contract tests for store/get/list/delete and reopen behavior, requires the headless Ubuntu CI leg to exercise the authenticated encrypted fallback, verifies the explicit v1 fallback envelope rejects unsupported versions fail-closed, retains AEAD tamper-detection coverage, and documents the credential-storage trust boundary in `SECURITY.md`. The v1 envelope is TauTerm’s first persisted fallback credential format, so there is no predecessor on-disk TauTerm format to migrate.

### Protocols
- **Local Shell (native PTY)** — adds cross-platform local terminal sessions backed by ConPTY on Windows and Unix PTYs on Linux/macOS. Shell selection supports automatic platform detection or a custom executable, an independent argument list and a working directory (user home by default). Windows discovery includes native PowerShell/CMD, WSL default and per-distribution presets, Git Bash, MSYS2/Cygwin Bash and NuShell; managed WSL launcher arguments stay separate from user arguments, Linux working directories default to `~`, and the native Auto order never enters WSL implicitly. Sessions use UTF-8 with `TERM=xterm-256color`, support terminal resize/search/logging, and intentionally exclude SendBar, file transfer and protocol tools.
- **Multi-terminal Local Shell and Windows elevation** — one saved Local Shell configuration can open up to 32 active independent PTYs through the same parent/child model as SSH. Native Windows shells expose a per-child `New (as Administrator)` action backed by a one-shot elevated helper and paired one-way local named pipes; the GUI remains unelevated, WSL is excluded, UAC cancellation creates no child, and elevated children carry a shield marker.

### Changed
- **Structured terminal disconnect lifecycle** — terminal channels now report user disconnect, remote EOF, I/O failure, device removal and process exit as structured reasons. User-requested disconnects and successful Local Shell exits clear the terminal, while abnormal disconnects and non-zero exits retain the in-memory terminal screen with the reason visible until reconnect, deletion or app exit.
- **Localized and bounded Local Shell presentation** — the connection type and configuration heading now follow the active UI language, new unnamed sessions use `Shell @ <resolved type>`, and Local Shell publishes its resolved starting path (or `~` for the default WSL home) as the standard session endpoint. The protocol-agnostic sidebar renders it like every other session; all titles and endpoints shrink and ellipsize within the card, with full values available through tooltips.
- **Elevated Local Shell startup and disconnect reliability** — command and event traffic now use separate one-way named pipes so a blocking output read cannot stall input or shutdown. Terminal bytes arriving before xterm mounts are retained in a bounded per-session startup buffer, and child removal is reflected immediately while backend cleanup completes.
- **Elevated Local Shell access after UAC** — fixed `ERROR_ACCESS_DENIED (os error 5)` after accepting elevation. The two logical one-way pipes now use duplex-capable server handles so `SetNamedPipeHandleState` can restore blocking mode, while helper handles remain direction-limited; a production-mode pipe regression test covers the exact failure.
- **Repeatable icon production contract** — every future functional icon must be generated from its registered semantic row with fixed family reference images through `npm run prompt:icon`. Strict validation now keeps the semantic table, runtime registry and PNG set aligned and rejects dark, violet/indigo and off-palette assets; the preview board pins family anchors beside 12/14/18/24px dark/light review sizes.

### Developer Experience
- **Protocol-neutral terminal child factory** — SSH and Local Shell now expose terminal creation through `SessionChannelFactory`; the session store owns common numbering, limits, I/O, statistics, exit-history and parent lifecycle behavior while each protocol keeps resource construction behind its adapter.

## [0.5.1] — 2026-09-01

### Changed
- Bumped TauTerm to v0.5.1 as a minimal patch release for end-to-end validation of the signed online update path from v0.5.0.
- No intentional user-facing product changes; this release isolates updater behavior.

## [0.5.0] — 2026-08-31

### Security
- **Credential storage backends** — prefer the operating-system keyring (Windows Credential Manager, macOS Keychain, or Linux Secret Service); when it is unavailable, store credentials in an Argon2id-derived AES-256-GCM vault that must be unlocked for the current app session. SSH connection-form passwords are not persisted automatically.
- **Least-privilege virtual ports** — Windows keeps the main app non-elevated and delegates com0com operations to the LocalSystem service; Unix uses an in-process PTY bridge.
- **On-demand elevation for com0com (Windows)** — the app no longer requests full administrator rights on every launch: the `requireAdministrator` manifest is removed (the app now runs `asInvoker`), and privileged com0com operations (driver install, virtual COM port pair create/remove/cleanup) are delegated to a new LocalSystem Windows service (`tauterm-service`) over a named pipe with a narrow typed API and caller identity verification. The main app stays non-elevated and exits cleanly without leaving orphaned port pairs — the service tracks per-client ownership and auto-cleans on pipe close / crash. When the service is unavailable (dev / portable), the app falls back to the previous on-demand UAC path, with fixes for unreliable success detection and the hardcoded bus-0 driver install.
- **Privileged service pipe client verification hardened** — `TauTermService` now requires the connecting client to reside in the same directory as the service (the install directory) in addition to the `tauterm.exe` name check, closing a rename-based spoofing vector where any binary renamed to `tauterm.exe` could issue narrow SYSTEM operations.

### Platforms
- **Native Unix PTY bridge** — Linux and macOS no longer require `socat`, a shell `PATH` entry, a `/tmp` symlink, or an external helper process for virtual serial endpoints.

### Protocols
- **TFTP exposure confirmation** — starting a server that listens on a non-loopback interface with remote writes and overwrite enabled now requires explicit user confirmation; existing defaults remain unchanged for compatibility.
- **Telnet (RFC 854)** — adds a Telnet terminal session with full option negotiation (NAWS window-size, Suppress-Go-Ahead, Transmit-Binary) handled by the `telnet` crate, TCP keepalive and a bounded connect timeout, and local-echo state surfaced to the UI via `telnet-echo-state` so the front-end stays in sync with the server's echo mode. Sync I/O; no file transfer (by design).
- **iperf network bandwidth test (iperf2 & iperf3)** — adds a network throughput plugin supporting both iperf2 (self-implemented protocol) and iperf3 (wire-compatible via `riperf3`); a single session acts as both client (transient config → run → result → end) and server (listens for external iperf clients, e.g. a board). Configurable duration, parallel streams, report interval, bandwidth cap, TCP window, plus iperf2 dualtest/tradeoff and iperf3 reverse/bidir/omit. iperf2 and iperf3 are not wire-interoperable — both ends must run the same version.
- **Session-level character-set transcoding** — text-path sends (SendBar / keyboard / Lua `send_text`) are transcoded from UTF-8 to the session encoding (GBK, GB18030, Big5, Shift-JIS, EUC-JP, EUC-KR, ISO-8859-1 → windows-1252); HEX sends and raw Lua `send` pass through unchanged; received bytes are decoded back to UTF-8 so text-format session logs stay readable. Un-mappable characters are substituted with `?` rather than an HTML numeric reference.
- **journald log viewer (SSH)** — runs `journalctl -o json` over the SSH exec channel for three modes: real-time streaming (`journald:entry` events), cursor-based paged history queries, and streamed JSON-file export with progress/complete/error/cancel events; active operations are tracked in a panic-safe session registry.
- **Network Debug (TCP/UDP)** — merges the planned TCP Raw and UDP Monitor into a single network debug session:
  - TCP Client / TCP Server (multi-client with configurable cap); UDP Client = fixed send target on an unconnected socket (sends to a fixed remote, receives from any source incl. broadcast/multicast), UDP Server = single connectionless session (local bind, unicast / broadcast / multicast with IGMP join, TTL, interface, self-receive)
  - TCP uses a container session + per-peer channel model (RFC 4254 Channel Mechanism): each client gets its own I/O loop, stats, CommHandle (encoding / auto-reply / Lua scripts isolated per peer), logging and `session-data` stream; **UDP is peerless** — a single socket `recv_from` emits each datagram as a `session-data` event carrying the source address
  - **Peer navigation (TCP) moved into the left session tree** (non-tab child nodes with status dot); peers are auto-named "Peer N" like SSH's "Channel N" (address shown as the second line); clicking a peer routes to the container view and selects it; clicking the container deselects; right-click offers Disconnect / Remove tombstone
  - Endpoint addresses are uniform across roles with a transport prefix (`tcp://host:port` / `udp://host:port`); disconnected sessions show a blank data area like terminal sessions (state conveyed by the status bar)
  - TCP Client is a plain single session (auto-selects its only peer, disconnects when the peer leaves); UDP Client is a fixed-remote single session with no peer tree and shows its own bound ip:port in the endpoint line
  - **Bare view (serial-like)**: no in-view header or toolbar — identity lives in the sidebar, TX/RX stats in the status bar; **data mode (Dual/Text/Hex) is TCP-only** (connect-time `data_mode` param, no in-session switch); UDP always shows a dual HEX+ASCII per-datagram grid
  - **SendBar moved to the global bottom position**, with a **unified send-target bar (`TargetBar`) shared across all four send modes** (basic / command / auto-reply / script); TCP Server shows a peer dropdown with an "all clients" pseudo-target for broadcast, UDP Server shows a manual-target address input with a recent-source quick-reply dropdown; broadcast / multicast addresses are typed manually (e.g. `255.255.255.255:port`); `SessionContext.sendToTarget` routes the current target for frontend sends
  - **Broadcast-to-all (TCP) is now a send-target value** (the "all clients" option in the target bar) rather than a separate mode-switcher toggle; SSH multi-channel broadcast is removed; UDP broadcast/multicast is network-layer via the manual target address; the script engine's `send()`/`send_text()` route to the synced current target, with new `send_to(target, data)` / `send_to_text(target, data)` Lua functions for explicit UDP targets
  - Dual / Text / Hex display for TCP streams (delimiter + timeout framing), per-datagram packet grid for UDP showing the full source/target timeline (datagram boundaries preserved, no per-source filtering)
  - Peer lifecycle hardening (TCP): two-phase `close_sub_connection` (signal under lock, join outside) eliminating a lock-cycle deadlock; peer disconnects settle state + final stats and emit `netdbg-peer-left` with final TX/RX bytes; TCP Server cap counts only connected peers (disconnected tombstones free their slots)
  - `max_clients` configurable in the connect dialog (TCP Server)
  - Manual send supports TEXT/HEX toggle; TX bytes counted into session stats; send failures surfaced via toast
  - Multicast group validated as IPv4 multicast (224.0.0.0–239.255.255.255) with inline hint
  - No file transfer (by design), no auto-reconnect / heartbeat (debugger must observe disconnects)

### UI
- **Sidebar session cards: clearer separation & tighter left alignment** — unselected cards now carry a persistent subtle border (`--glass-border-default`) so long session lists read as distinct items instead of one text block (hover/active states unchanged); left-side whitespace trimmed (expand-arrow placeholder 16px → 10px, list/container padding reduced, text starts ~15px closer to the panel edge); card vertical spacing 2px → 4px; endpoint line font-size tokenized (`--text-xs`); header/search/cards/settings-button left edges unified on one alignment baseline; child-tree indent 16px → 12px
- **New-session dialog: responsive mode grid** — the connection-type card grid switches from a fixed 2 columns to `auto-fit` so it flows to 3 columns (and more as space allows); the dialog width is now adaptive (`min(560px, 100vw - 32px)`); card labels reserve two lines so every card keeps a uniform size when a label wraps
- **Default window size 1440×900 with small-screen clamping** — the initial window is now 1440×900 (up from 1200×800) for a wider terminal view; on launch the window is clamped to the primary monitor's work area (Windows taskbar excluded) and centered, so screens smaller than the default (e.g. 1366×768) no longer open with controls or edges off-screen.
- **File Manager: list/grid view toggle + icon toolbar** — the SFTP file manager now offers an icon-tile (Windows-style "tiles") view alongside the existing list view, with the choice persisted globally via localStorage; a new inline icon toolbar surfaces the previously context-menu-only actions (refresh / new file / new folder / upload) plus a single view-toggle button (drag-handle icon) with a `☰`/`⊞` glyph and an action-style accessible label (switch to grid / switch to list). The `..` parent-directory entry now behaves like a folder (single-click selects, double-click navigates) and shares its icon with regular folders via a single source; file icons are extension-category emoji shared by both views. Rows/tiles expose grid ARIA semantics (`row`/`gridcell`, `aria-selected`, Enter/Space).

### Fixed
- **Icon system semantic and small-size audit** — all 61 registered PNG assets now have an explicit production meaning and a 12px readability contract. Directional arrows, window controls, sidebars, file views, send mode and action steps now use dedicated assets; XMODEM no longer implies wireless activity; the file-manager control no longer presents a drag handle; status, log, copy/paste and transfer meanings are separated. The registry has no inline-SVG escape hatch, while CSS remains reserved for connection-status dots.
- **Security settings localization and feedback** — the security panel localizes known backend labels and uses a safe localized fallback for unknown backends, while handling loading, refresh, unlock, lock, and status failures without exposing backend error text.
- **Security settings icon and themed controls** — adds the security lock icon and aligns refresh, unlock, and lock controls with the shared secondary glass-button layout across themes.
- **com0com: test server and TauTermService now coexist without clobbering each other** — `scripts/test-serial-session.py` previously created its virtual port pair on the lowest available bus/ports (`COM22/COM23`), which could collide with the product's own pairs and be wiped by the service's startup orphan cleanup. It now lives in a dedicated reserved region (`COM200-COM255` / bus `200-255`, fixed pair `COM200↔COM201`): the product's `VirtualPortManager` never allocates buses in that range (so the service startup orphan cleanup also leaves them alone), skips that port range when scanning and, when the test's reserved ports push the scan start into the range, wraps back to the low port range so it still finds free pairs instead of reporting none. `--teardown-all` only touches reserved-segment buses. A new build-time `check-reserved-region.js` keeps the Python and Rust constants in sync.
- **com0com: virtual port failure message was misleading and unlocalized** — when virtual port pair creation failed, the app showed a hard-coded English "com0com driver not installed — run TauTerm as administrator once, or reinstall the application" regardless of the actual cause (port exhaustion, UAC cancellation, …). The `virtual-port-failed` event now carries the real failure reason plus a coarse `kind` (`files_missing` / `driver_missing` / `permission` / `create_failed`), and the status bar renders the localized message (with New i18n keys `permissionRequired` / `createFailed`) while keeping the raw reason in the tooltip.
- **Network Debug: background receive lost for inactive sessions** — the `session-data` listener resolved peers from a ref that is only refreshed while the view renders; an inactive (unmounted) session froze that ref to an empty list, so all data arriving in the background was dropped and the data area appeared empty on switch. The listener now resolves peers from `SessionContext.stateRef` (always fresh), so frames accumulate per session even while inactive and are visible immediately on switch.
- **Network Debug: server data area stays empty after tab switching** — the `session-data` listener was registered once per session by the plugin session store, so its closure kept the peer refs from the first mount; after remounting (tab switch) those refs froze at the initial empty list and the server view never matched its peers. The listener now resolves the current refs from a per-render registry, so incoming data is routed to the correct container's frame store again.
- **Network Debug: client shows its own bound address** — peers now carry a `local_addr` (client's OS-assigned socket address after connecting); a TCP/UDP client session card appends it to the endpoint line (`tcp://127.0.0.1:8080 · 127.0.0.1:56780`, single line with ellipsis), letting you map the client to the matching "Peer N" row on the server side without changing the card height.
- **Non-Windows builds failed after the service bundle change** — `bundle.resources` referenced `binaries/tauterm-service.exe` unconditionally while the placeholder was only created on Windows, so Linux/macOS `cargo build` broke in `tauri-build`'s resource check. The service binary resource now lives in `tauri.windows.conf.json` and is only bundled on Windows.
- **Elevated batch could keep running after a timeout** — `run_elevated` returned a timeout error but never terminated the elevated `cmd.exe`, which could still create/remove port pairs in the background (UI shows failure while ports change); it now calls `TerminateProcess` on timeout, and the batch timeout was raised to 120s to cover multi-bus two-stage cleanup delays.
- **NSIS hooks ignored `sc.exe` exit codes** — a stale `TauTermService` registration (e.g. from an interrupted upgrade) silently left the virtual port feature broken; the installer now rebuilds an existing registration and surfaces registration/start failures with a message box.
- **`TauTermService` stop could hang** — with no client connected the service blocked forever in `ConnectNamedPipe`, so `sc stop` / shutdown / uninstall stalled up to the SCM timeout; a shutdown watcher thread now self-connects the pipe on STOP to unblock the wait (the self-connection is rejected by client verification), so the service exits promptly.
- **Client pipe I/O had no timeout** — a wedged service could stall app startup or operations indefinitely; named-pipe reads/writes now use overlapped I/O with a 60s timeout (service crash still fails fast via pipe close).
- **Uninstall no longer leaves a residual `C:\ProgramData\TauTerm` folder** — the privileged `TauTermService` previously persisted com0com bus bookkeeping (`com0com_state.json`) under ProgramData and, holding that directory while it exited, could not be removed synchronously (surfaced as an empty-folder residual by Geek/NSIS right after uninstall). `VirtualPortManager` now supports a stateless mode and the service uses it: it writes no state to disk (no ProgramData directory is created), orphan port pairs are cleaned lazily from the driver's real state and per-client on pipe close, and the NSIS uninstall hook no longer has to wait out a service-held directory lock.

### Developer Experience
- Remove Claude Code skill shims — `.agents/skills/` is now the sole canonical skill location. Deleted `scripts/gen-skill-shims.mjs`, the `skills:sync` npm script, the `.claude/` directory (including `settings.json`), and the CI drift check.

### Release process
- **Updater and release validation hardening** — release dispatch accepts one version input, derives pre-release status from `-alpha.N`, `-beta.N`, or `-rc.N`, validates the exact five updater targets and signatures, verifies tag-scoped assets before promoting a stable release to `latest`, and cleans up the release created by that run (whether draft or temporarily public) and its tag when final validation fails.

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
