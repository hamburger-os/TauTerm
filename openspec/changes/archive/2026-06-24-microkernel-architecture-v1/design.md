## Context

TauTerm v0.2 is a serial-port-only terminal emulator built on Tauri v2 (Rust + React/TypeScript + xterm.js). The current architecture has a fundamental limitation: `spawn_io_thread` binds to `Box<dyn serialport::SerialPort>`, `connect_session` hard-codes `ConnectionType::Serial`, and the transfer subsystem only supports port-handoff to serial devices. To compete with WindTerm and MobaXterm, TauTerm must support SSH, Telnet, TCP Raw, TRDP, FTP, NFS, iPerf, and more—each with fundamentally different I/O models (blocking serial, async TCP, encrypted SSH channels, UDP datagrams).

The codebase is at an optimal inflection point: ~3,000 lines of Rust, only one protocol implemented, clean session persistence, and a well-structured React frontend. A microkernel refactor now costs far less than after adding 5+ protocols on top of the current serial-bound architecture.

## Goals / Non-Goals

**Goals:**
- Design a microkernel that provides 8 services (Window, Tab, IPC, Config, Plugin, Theme, Shortcut, i18n) with zero protocol awareness
- Define a `ProtocolAdapter` trait that any session type can implement to become a first-class TauTerm plugin
- Abstract I/O into a `Channel` trait that serial ports, TCP streams, SSH channels, pipes, and UDP sockets all satisfy
- Support dual-mode I/O execution: sync (`std::thread`) for serial/pipe and async (`tokio`) for TCP/SSH/HTTP
- Design a three-strategy transfer subsystem: Inline (serial handoff), SideChannel (SSH SFTP/SCP), SeparateConnection (FTP)
- Implement a Credential Store with keyring-backed encryption and AES-256-GCM fallback
- Provide a frontend plugin registration API via `registerPlugin()` that delivers connect forms, toolbar items, context menus, and bottom panels
- Render any session type in a unified tab bar using a `content_type` adapter pattern (terminal / file_browser / stats_dashboard / custom)
- Deliver a comprehensive README.md that serves as the high-level architecture blueprint for all future development

**Non-Goals:**
- Third-party plugin marketplace (v1.0+ concern; this design only covers the plugin *architecture*, not distribution)
- Plugin hot-reloading or dynamic linking (initial implementation uses compile-time built-in plugins)
- Multi-window support (architecture allows it but not implemented in this change)
- Terminal session recording and playback
- Cloud sync of sessions/credentials

## Decisions

### D1: Microkernel over Layered Monolith

**Chosen**: 8-module microkernel where each module has a single responsibility and well-defined interface.

**Alternatives considered**:
- *Layered architecture (keep current pattern)*: Rejected because layers inevitably leak protocol concerns upward. The current `commands.rs` already mixes protocol-specific logic (YModem send/receive) with session management.
- *Actor model (Actix)*: Rejected for complexity. Tauri's state management + `Mutex` provides sufficient thread safety without an actor framework.

**Rationale**: The microkernel enforces a hard boundary between "platform" (window, tabs, config, shortcuts) and "features" (protocols, transfer, tools). Adding a new protocol plugin never touches kernel code—only `impl ProtocolAdapter` and `registerPlugin()`.

### D2: Trait-based ProtocolAdapter over Enum-based SessionImpl

**Chosen**: `ProtocolAdapter` trait with `Channel` return type.

**Alternatives considered**:
- *Keep `SessionImpl` enum (current approach)*: Rejected because adding a protocol requires modifying the enum, all match arms, and the session manager—violating the Open/Closed Principle.
- *Trait objects with `dyn TermSession`*: Considered but rejected because the previous attempt had stub methods. The new `ProtocolAdapter` trait is deliberately small (5 methods), avoiding the "fat trait" problem.

**Rationale**: A 5-method trait (`connect`, `disconnect`, `discover_endpoints`, `content_type`, `transfer_protocols`) is the minimum viable interface for protocol diversity. Each method returns concrete, well-typed values. The `Channel` trait abstraction ensures the I/O engine never knows what transport it's driving.

### D3: Dual-Mode I/O (sync + async)

**Chosen**: Plugin declares `IoStrategy::Sync` or `IoStrategy::Async`. Kernel provides both a `std::thread`-based I/O loop and a `tokio::spawn`-based I/O loop.

**Alternatives considered**:
- *All-async*: Rejected because serial ports on Windows have poor async support. Blocking serial I/O on a dedicated thread is the proven pattern.
- *All-sync*: Rejected because SSH and HTTP cannot be practically implemented without async. Blocking on network I/O would freeze the UI.

**Rationale**: Different protocols have different I/O natures. Serial is fundamentally blocking-by-byte. TCP/SSH is fundamentally event-driven. Forcing either model onto the other creates impedance mismatch. The dual-mode approach respects each protocol's natural I/O pattern while sharing the same stats collection, cancellation, and event emission infrastructure.

### D4: Three-Strategy Transfer over Single Strategy

**Chosen**: `TransferManager` selects Inline (port handoff), SideChannel (protocol sub-channel), or SeparateConnection (new connection) based on the session's protocol capabilities.

**Alternatives considered**:
- *Keep Inline only*: Rejected because SSH SFTP opens a separate channel within the SSH session—there's no "port" to hand off.
- *Unified streaming transfer*: Considered but rejected because the control flow differs fundamentally. YModem over serial requires exclusive port access; SFTP over SSH multiplexes over the existing connection; FTP requires a separate data port.

**Rationale**: Each strategy maps to a real protocol constraint. Inline = the transport IS the data channel (serial). SideChannel = the transport multiplexes a data channel (SSH). SeparateConnection = the control and data channels are separate (FTP). Trying to unify these into one abstraction would produce a leaky, complex interface.

### D5: Keyring + AES-256-GCM Credential Storage

**Chosen**: Primary backend = OS keyring (macOS Keychain / Windows Credential Manager / Linux Secret Service via `keyring-rs`). Fallback = AES-256-GCM encrypted file in app data directory.

**Alternatives considered**:
- *Plain file only*: Rejected for security—terminals routinely handle root passwords and SSH private keys.
- *Keyring only (no fallback)*: Rejected because headless Linux systems may lack a secret service daemon.

**Rationale**: Defense in depth. The keyring provides OS-level protection (unlocked only when user is logged in). The AES fallback ensures functionality on minimal systems, with encryption at rest using a machine-derived key.

### D6: Content Type Adapter Pattern for Unified Tabs

**Chosen**: `CONTENT_RENDERERS` map keyed by `content_type` string, populated by plugins at registration time.

**Alternatives considered**:
- *Separate tab bar per session type*: Rejected—defeats the purpose of a unified terminal emulator. Users expect to see Serial, SSH, and FTP sessions side by side.
- *Iframe isolation per plugin*: Rejected for performance and integration complexity. xterm.js instances are expensive; sharing them in a pool with CSS visibility toggling (current approach, proven performant) is better.

**Rationale**: The adapter pattern gives each plugin full control over its visual representation while the kernel manages tab lifecycle (create, focus, close, reorder, drag-out). This is exactly how VS Code's tab system works with different editor types.

## Risks / Trade-offs

- **[Risk] Trait object overhead for `dyn Channel`**: Each I/O operation goes through dynamic dispatch. → **Mitigation**: I/O is the dominant cost (microseconds to milliseconds); vtable lookup (~2ns) is negligible. The `Channel` trait methods are large-grained (read/write/flush), not per-byte calls.

- **[Risk] Dual-mode I/O increases kernel complexity**: Two I/O loop implementations to maintain. → **Mitigation**: Extract shared logic (stats counters, cancellation flags, event emission) into free functions used by both loops. The sync and async loops differ only in the read/write dispatch mechanism.

- **[Risk] Plugin system design may be wrong for unknown future protocols**: The `ProtocolAdapter` trait might not fit a protocol we haven't imagined yet. → **Mitigation**: The trait is deliberately minimal (5 methods). If a protocol needs something fundamentally different, it can use `content_type: "custom"` and render its own UI while still benefiting from the kernel's tab management, config storage, and shortcut system.

- **[Risk] Breaking change scope is large**: Every existing Rust module and several React contexts change. → **Mitigation**: Phase the implementation. Phase 1: Extract kernel modules without changing behavior. Phase 2: Introduce `Channel` trait, refactor Serial to implement it. Phase 3: Add Plugin Host, convert Serial to built-in plugin. Phase 4: Add new protocols. Each phase is independently testable.

- **[Trade-off] Compile-time plugins vs. dynamic loading**: Built-in plugins are simpler but require recompilation to add a protocol. → This is acceptable for v0.x. The plugin *architecture* supports future dynamic loading; the `ProtocolAdapter` trait and `registerPlugin()` API don't change when we move from compile-time to load-time registration.

## Open Questions

1. **UDP/iPerf as a "session"**: UDP is connectionless. Should iPerf/UDP sessions appear in the tab bar like terminal sessions, or as a separate "tool" panel? Recommendation: Tab bar, with `content_type: "stats_dashboard"` — the tab hosts the iPerf control panel and results.

2. **Local shell (PTY) vs. remote shell (SSH)**: Should TauTerm's "Shell" plugin be a local PTY (like Windows Terminal) or an SSH shell, or both? Recommendation: Two plugins — `shell-local` (PTY, `content_type: "terminal"`) and `ssh` (remote, `content_type: "terminal"`). They share the terminal renderer but have different `ProtocolAdapter` implementations.

3. **NFS integration depth**: Full NFS client implementation is a massive undertaking. Should TauTerm aim for NFS mount browsing (read-only file browser over NFS) or full NFS client? Recommendation: Start with read-only browsing (`content_type: "file_browser"`) using an existing Rust NFS library, scope full read-write for later.
