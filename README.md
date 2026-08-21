# TauTerm

> **A fast, modern, cross-platform terminal emulator — built for network engineers and embedded developers.**

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE)
[![Release](https://img.shields.io/badge/release-v0.4.0-brightgreen.svg)](https://github.com/hamburger-os/TauTerm/releases)
[![Platform](https://img.shields.io/badge/platform-Windows%20%7C%20macOS%20%7C%20Linux-lightgrey.svg)](#)
[![Framework](https://img.shields.io/badge/Tauri-v2-67D6F8.svg)](https://tauri.app)

Built on **Tauri v2** (Rust + React + TypeScript), TauTerm combines a modern native-feeling UI with a microkernel plugin architecture: the kernel ships no protocol logic — every session type (Serial, SSH, Telnet, TFTP, FTP, iPerf…) is an independent plugin.

**中文用户请见 [README.zh-CN.md](README.zh-CN.md)。**

<!-- TODO(screenshots): hero screenshot of the main window -->

---

## Why TauTerm?

If you spend your day in terminals — jumping between production servers over SSH, flashing firmware over a serial port, or both — TauTerm is built for you:

- **Free and open source** — dual-licensed MIT / Apache-2.0, actively maintained
- **Lightweight native feel** — Tauri v2 (Rust + WebView2), an 8.0 MB installer
- **Two workflows, one app** — production-grade SSH/SFTP/TFTP/iPerf for ops; serial/YModem/Modbus/Lua for embedded
- **Extensible by design** — microkernel plugin architecture; every protocol is an independent plugin
- **Security-first defaults** — OS-native keyring, host-key fingerprint confirmation, log redaction
- **Modern design** — Liquid Glass v3 design system, three themes, fluid animations

---

## Features

### 🖥️ For Network Engineers

- **SSH + SFTP in one connection** — terminal and file transfer multiplexed over a single session (SideChannel architecture, no second login)
- **SFTP file manager** — remote directory browsing, upload/download, batch delete, rename, properties, breadcrumb navigation
- **TFTP server & client** — RRQ/WRQ serving, GET/PUT client, RFC 7440 windowing, CRC32 verification, tunable timeouts, exponential-backoff retry
- **Telnet** — RFC 854 option negotiation (ECHO/SGA/BINARY/NAWS), local-echo adaptation, live window-size sync, keepalive
- **iPerf speed testing** — iperf2 (own wire-compatible implementation) + iperf3 (vendored riperf3); TCP/UDP, `-t/-b/-P/-i/-w`, bidirectional modes, live rate curves and history
- **Remote journald viewer** — streaming follow and history query over SSH, level/keyword/unit filters, cursor pagination, JSON export with progress and cancel
- **Network debug session (TCP/UDP)** — TCP client / TCP server (multi-client cap) / UDP client & server (unicast, broadcast, multicast) in one session; TCP peers are isolated channels with their own I/O loop, stats, encoding/auto-reply/Lua scripts, logging and data stream; UDP is a single connectionless session with a per-datagram HEX+ASCII grid; TCP per-peer Dual/Text/Hex view, a unified send-target bar across all four send modes (per-peer / all-clients broadcast / UDP manual address with recent-source quick reply), TEXT/HEX manual send, `max_clients` cap
- *Planned (v0.6–v0.7): SSH tunnel & port-forwarding UI, jump hosts, session tree with groups, local shell, FTP, session recording*

### 🔌 For Embedded Developers

- **Serial (RS-232/485)** with a **virtual COM bridge** — auto-created port pairs (com0com) let external tools tap the physical link live; orphan-port auto-cleanup
- **XModem / YModem / ZModem** transfers, protocol chosen per session, inline handoff from the live serial port
- **Session charset transcoding** — UTF-8, GBK, GB18030, Big5, Shift-JIS, EUC-JP, EUC-KR, ISO-8859-1; receive stream decode, send-direction auto-encode, Dual-mode decode, always-readable UTF-8 logs
- **Dual text/HEX display** — draggable split with ASCII + HEX side by side, millisecond timestamps, `\r\n`/`\n`/`\r` auto-framing, TX/RX color coding
- **Four-mode send bar** — basic send (text/HEX, line endings, loop, history), command panel (predefined sequences, drag-sort, loop), auto-reply (visual rules, 5 match modes, 10 dynamic macros, timers), script editor
- **Embedded Lua 5.4 scripting** — per-session isolated VMs, sandboxed; `send()` raw byte passthrough, `send_text()` session-encoding-aware (Chinese text over GBK devices stays intact), and `send_to()`/`send_to_text()` for explicit UDP targets (broadcast/multicast)
- **Embedded toolbox** — CRC8/16/32 with presets, Base64/HEX/float/endianness conversion, bit ops, C `sizeof` calculator, Modbus RTU/ASCII and AT-command parsers
- *Planned (v1.1): waveform plotting engine with FFT, FireWater/JustFloat protocol compatible — works with existing firmware data formats out of the box*

### ⚡ For Everyone

- **Liquid Glass v3 design system** — three themes (Google Glow / Obsidian / Frosted), Framer Motion animations
- **Unified tabbed sessions** — serial, SSH, FTP, iPerf share one tab bar; offline session profiles, right-click reconnect
- **Credential store** — OS-native keyring (Windows Credential Manager / macOS Keychain / Secret Service) with AES-256-GCM file fallback
- **Security-first defaults** — SSH host-key fingerprint confirmation on first connect, key-change warnings, log redaction of passwords/keys/tokens, agent forwarding off by default
- **Auto-update** — Tauri updater, configurable check frequency, one-click install & restart
- **Command palette, searchable terminal, fully rebindable shortcuts** — `Ctrl+Shift+P` palette, `Ctrl+F` buffer search, click-to-record key rebinding
- **Session data logging** — text/hex/dual formats, rotation and expiry cleanup, one-click start/stop, live status-bar indicator
- **i18n** — zh-CN / en-US, plugin namespaces, runtime switching

---

## Performance

A lightweight footprint is a first-class goal — every release is validated against a performance budget (cold start, idle memory, scroll throughput).

| Metric | v0.4.0 (measured) |
|---|---|
| Windows installer (`x64-setup.exe`, incl. com0com driver) | **8.0 MB** |
| App binary | **25.7 MB** |
| Cold start & idle memory | Benchmarks published with the v0.8 performance-acceptance milestone |

*Measured on Windows 11 x64. The installer includes the com0com virtual serial driver.*

---

## Quick Install

### Windows

Download the latest installer from [GitHub Releases](https://github.com/hamburger-os/TauTerm/releases) (`TauTerm_x.x.x_x64-setup.exe`).

The installer bundles and auto-installs the open-source [com0com](https://com0com.sourceforge.net/) virtual serial driver (GPL v3.0.0.0, source available) so the virtual COM bridge works out of the box. Uninstalling TauTerm removes the driver as well.

### macOS / Linux

Prebuilt packages are on the way. For now, build from source — see [docs/BUILDING.md](docs/BUILDING.md).

---

## Protocol Support

| Protocol | Status | Content | Transfer | I/O |
|---|---|---|---|---|
| **Serial** (RS-232/485) | ✅ | terminal | YModem / XModem / ZModem (inline) | Sync |
| **SSH** | ✅ | terminal | SFTP (side-channel) | Async |
| **TFTP** | ✅ | custom | Standalone UDP engine | Headless |
| **Telnet** | ✅ | terminal | — | Sync |
| **iPerf2 / iPerf3** | ✅ | custom | Standalone engine | Async |
| **Network Debug** (TCP/UDP) | ✅ | custom | — | Sync |
| **Shell Local** (PTY) | 📋 v0.6 | terminal | — | Sync |
| **FTP** | 📋 v0.7 | file_browser | FTP (separate connection) | Async |
| **TRDP** | 📋 v1.0 | terminal | — | Async |
| **NFS** | 🔮 | file_browser | NFS (separate connection) | Async |

---

## Roadmap

```
v0.5  Credentials & security wrap-up: keyring persistence,
      log redaction in the production pipeline
v0.6  Ops essentials: Shell Local (PTY), session tree & grouping,
      SSH tunnel UI + jump hosts, Agent Forwarding,
      asInvoker privilege downgrade + UAC elevation helper
v0.7  Network debug session polish, FTP,
      session recording, split panes
v1.0  GA: full Windows validation, macOS/Linux core availability,
      performance budget met, plugin SDK docs, TRDP
v1.1  Waveform engine — "terminal + oscilloscope": WebGL plotting,
      FFT, FireWater/JustFloat compatible
v1.x  Horizon (priority order): multi-window, dynamic plugin loading,
      plugin marketplace, NFS, cloud session sync
```

---

## Development

- **[docs/BUILDING.md](docs/BUILDING.md)** — environment setup, dev mode, production builds for all three platforms
- **[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)** — microkernel design, plugin architecture, I/O strategies, security model
- **[CONTRIBUTING.md](CONTRIBUTING.md)** — contribution guide

## License

TauTerm is licensed under either of:

- [MIT License](LICENSE)
- [Apache License, Version 2.0](LICENSE-APACHE)

at your option.

The Windows installer bundles the [com0com](https://com0com.sourceforge.net/) kernel driver as a separate third-party GPL component — its license is unaffected by TauTerm's dual licensing.

---

**TauTerm** — one terminal for the server room and the lab bench.
