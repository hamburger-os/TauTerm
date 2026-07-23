# Changelog

All notable changes to TauTerm will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
