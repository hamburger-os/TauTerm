<p align="center">
  <img src="src/assets/icons/logo.png" width="112" alt="TauTerm logo">
</p>

<h1 align="center">TauTerm</h1>

<p align="center"><strong>One terminal for the server room and the lab bench.</strong></p>

<p align="center">
  Open-source desktop workspace for SSH/SFTP, serial and TCP/UDP network debugging — built with Rust + Tauri.
</p>

<p align="center">
  <a href="https://github.com/hamburger-os/TauTerm/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/hamburger-os/TauTerm/actions/workflows/ci.yml/badge.svg?branch=master"></a>
  <a href="https://github.com/hamburger-os/TauTerm/releases"><img alt="Release" src="https://img.shields.io/github/v/release/hamburger-os/TauTerm?include_prereleases&label=release"></a>
  <a href="LICENSE"><img alt="License: MIT OR Apache-2.0" src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg"></a>
  <img alt="Rust + Tauri" src="https://img.shields.io/badge/Rust%20%2B%20Tauri-v2-24C8DB">
</p>

<p align="center">
  <a href="https://github.com/hamburger-os/TauTerm/releases"><strong>Download</strong></a>
  · <a href="docs/SUPPORTED_PLATFORMS.md">Supported platforms</a>
  · <a href="docs/BUILDING.md">Build from source</a>
  · <a href="docs/ARCHITECTURE.md">Architecture</a>
  · <a href="README.zh-CN.md">中文</a>
</p>

TauTerm brings **remote access, embedded bring-up and network debugging** into one lightweight desktop app. It is designed for engineers who would rather not keep switching between an SSH client, an SFTP client, a serial terminal and separate TCP/UDP debugging tools.

> **Version note:** this README describes the current `master` branch. GitHub Releases are the packaged snapshots intended for end users and may lag behind `master`. See [CHANGELOG.md](CHANGELOG.md) and each release page for the exact set of shipped features.

![TauTerm SSH terminal and SFTP file manager in one session](docs/assets/hero-en.png)

---

## Why TauTerm?

| If you need… | TauTerm gives you… |
|---|---|
| Server access | SSH terminal + SFTP in one authenticated connection |
| Embedded bring-up | Serial RS-232/485, Text/HEX/Dual views, X/Y/ZModem |
| Network debugging | TCP/UDP client & server, TFTP, Telnet, iPerf |
| Repetitive-task automation | Per-session Lua 5.4 scripting and auto-reply rules |
| One consistent workspace | Unified sessions, logging, command palette and shortcuts |
| Room to extend | A protocol-oriented microkernel/plugin architecture |

TauTerm deliberately serves both **network engineers** and **embedded developers**. Serial communication is a first-class workflow rather than an afterthought, while SSH/SFTP and network-debugging tools share the same session-oriented desktop experience.

---

## Core workflows

### SSH + SFTP

One authenticated SSH session keeps the terminal, remote files and remote journald workflows together. The SFTP file manager supports browsing, upload/download, rename, batch delete and remote file inspection.

### Serial and embedded development

![RT-Thread serial terminal with file transfer and protocol tools](docs/assets/serial-rtthread-dual-en.png)

Use RS-232/485 with Text, HEX or Dual views, timestamps, TX/RX differentiation, character-set transcoding, X/Y/ZModem transfers, Lua automation, auto-reply rules and a developer toolbox for common binary/protocol tasks.

On Windows, TauTerm can bridge a serial session through bundled com0com virtual COM pairs. Linux and macOS use an in-process POSIX PTY bridge with no `socat` or external helper dependency.

### TCP/UDP and throughput testing

![TCP loopback receive log](docs/assets/network-tcp-loopback-en.png)

Run TCP client/server and UDP client/server workflows in the same app, including peer selection, per-peer statistics, Text/HEX inspection and scripted sending. TFTP, Telnet and iPerf2/iPerf3 complement the same network-debugging workspace.

---

## Highlights

### Network engineering

- **SSH + SFTP in one session** — terminal and file transfer share a single authenticated connection.
- **Network Debug (TCP/UDP)** — TCP client/server and UDP client/server with multi-peer handling, target selection and statistics.
- **TFTP** — client/server workflows with RFC 7440 windowing, CRC32 verification and retry controls.
- **Telnet** — RFC 854 negotiation, live window-size sync and keepalive.
- **iPerf2 / iPerf3** — TCP/UDP testing, bidirectional modes, live rate curves and history.
- **Remote journald viewer** — stream or query logs over SSH with filters and JSON export.

### Embedded development

- **Serial RS-232/485** with optional virtual serial bridging.
- **XModem / YModem / ZModem** transfers directly from an active serial session.
- **Text / HEX / Dual display** with timestamps, framing and TX/RX differentiation.
- **Character-set transcoding** — UTF-8, GBK, GB18030, Big5, Shift-JIS, EUC-JP, EUC-KR and ISO-8859-1.
- **Four-mode send bar** — manual send, command panel, auto-reply rules and scripts.
- **Embedded Lua 5.4** — isolated per-session VMs with raw-byte and encoding-aware send APIs.
- **Developer toolbox** — CRC, Base64/HEX, float/endianness conversion, bit operations, C `sizeof`, Modbus and AT-command parsers.

### Everyday workflow

- Unified tabbed sessions and offline connection profiles.
- Searchable terminal, command palette and fully rebindable shortcuts.
- Session logging with rotation and expiry cleanup.
- zh-CN / en-US runtime language switching.
- Three Liquid Glass themes: Google Glow, Obsidian and Frosted.
- Credential storage that prefers the OS keyring, with an Argon2id + AES-256-GCM vault fallback.

TFTP configurations that listen beyond the local machine while allowing remote writes and overwrites require explicit confirmation before the server starts.

---

## Protocol matrix

| Protocol | Current `master` | Content | Transfer / role |
|---|---:|---|---|
| **Serial** (RS-232/485) | ✅ | terminal | X/Y/ZModem inline |
| **SSH** | ✅ | terminal | SFTP side-channel |
| **TFTP** | ✅ | custom | client + server |
| **Telnet** | ✅ | terminal | RFC 854 |
| **iPerf2 / iPerf3** | ✅ | custom | network benchmark |
| **Network Debug** (TCP/UDP) | ✅ | custom | client + server |
| **Local Shell** (PTY) | 📋 planned | terminal | v0.6 target |
| **FTP** | 📋 planned | file browser | v0.7 target |
| **TRDP** | 📋 planned | terminal | v1.0 target |

---

## Install

### Windows

Download the newest **x64 NSIS installer** from [GitHub Releases](https://github.com/hamburger-os/TauTerm/releases).

The installer bundles the open-source [com0com](https://com0com.sourceforge.net/) virtual serial driver so the virtual COM bridge can work out of the box. com0com remains a separate GPL component. Windows ARM64 and MSI are not current release targets. Builds are not yet Authenticode-signed, so SmartScreen may warn on first install.

### Linux

Choose the x86_64 `.deb`, `.rpm` or `.AppImage` from [GitHub Releases](https://github.com/hamburger-os/TauTerm/releases). Release artifacts use Ubuntu 22.04 as the Linux build baseline.

The Linux virtual serial bridge is implemented with an in-process POSIX PTY and does not require `socat` or another helper process.

### macOS

Download the **Apple Silicon** `.dmg` / updater app archive from [GitHub Releases](https://github.com/hamburger-os/TauTerm/releases). macOS Intel is not a current release target.

macOS remains a **tech preview** and is not yet notarized, so Gatekeeper may require a one-time right-click → Open.

See [Supported Platforms](docs/SUPPORTED_PLATFORMS.md) for the exact architecture, package and signing matrix.

---

## Security & trust

TauTerm handles remote credentials, network traffic, serial devices, local files and software updates, so security boundaries are treated as product features rather than afterthoughts.

- SSH host-key verification and log redaction.
- Credential storage prefers the OS keyring; when unavailable, an Argon2id + AES-256-GCM vault can be unlocked for the app session.
- Passwords typed into the SSH connection form are not saved automatically.
- On Windows, the main app runs as a standard user; privileged virtual-port operations are delegated to a background service, with a development fallback path when that service is unavailable.
- Tauri updater artifacts are cryptographically signed and verified by the release pipeline.
- Final release artifacts are validated and receive GitHub build-provenance attestation before publication.

Please report suspected vulnerabilities privately as described in [SECURITY.md](SECURITY.md).

---

## Roadmap

| Target | Focus |
|---|---|
| **v0.6** | Local shell, session groups, SSH tunnels and jump hosts |
| **v0.7** | Network-debug polish, FTP, recording and split panes |
| **v1.0** | Release-grade cross-platform validation, performance budget, plugin SDK docs and TRDP |
| **v1.1** | "Terminal + oscilloscope": WebGL plotting, FFT and FireWater/JustFloat compatibility |

Roadmap versions are targets, not promises; priorities may change based on real user feedback. For shipped changes, use [CHANGELOG.md](CHANGELOG.md) and [GitHub Releases](https://github.com/hamburger-os/TauTerm/releases) as the source of truth.

---

## Architecture & contributing

TauTerm uses a **microkernel plugin architecture**. The kernel provides shared platform capabilities while protocol implementations register through a common adapter/manifest model. Read [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the current design.

Contributions are welcome:

- Found a reproducible bug? [Open a bug report](https://github.com/hamburger-os/TauTerm/issues/new/choose).
- Missing a feature that keeps you on another terminal? [Request it](https://github.com/hamburger-os/TauTerm/issues/new/choose).
- Want to contribute code? Read [CONTRIBUTING.md](CONTRIBUTING.md) and [docs/BUILDING.md](docs/BUILDING.md).
- Interested in protocol plugins? Start with the plugin architecture in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

If TauTerm is useful to you, starring the repository helps other network and embedded engineers discover it.

---

## License

TauTerm is available under either the **MIT License** or **Apache License 2.0**, at your option.

The Windows installer bundles [com0com](https://com0com.sourceforge.net/) as a separate third-party GPL component; its license is unaffected by TauTerm's dual licensing.

<p align="center"><strong>TauTerm — one terminal for the server room and the lab bench.</strong></p>
