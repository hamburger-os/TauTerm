# Supported Platforms

TauTerm deliberately distinguishes between **core protocol portability** and **release-grade platform support**. A platform is listed as supported only when TauTerm's release pipeline builds and validates an installer/package for that architecture.

## Current release targets

| Platform | Architecture | Release status | Package |
|---|---|---|---|
| Windows 10/11 | x86_64 | Supported | NSIS `.exe` |
| Linux | x86_64 | Supported | `.deb`, `.rpm`, `.AppImage` |
| macOS | Apple Silicon (`aarch64`) | Tech preview | `.dmg`, `.app` updater archive |

The Linux release ABI baseline is **Ubuntu 22.04**. The CI matrix currently validates Ubuntu 22.04, Windows 2025 and macOS 15.

## Not currently supported

- Windows ARM64
- Linux ARM64
- macOS Intel (`x86_64`)
- Windows MSI distribution

These combinations may compile partially, but they are not release targets and should not be advertised as supported.

## Virtual serial implementation

Windows uses the bundled com0com kernel driver and exposes a real virtual COM pair. Linux and macOS use an in-process POSIX PTY created by Rust's `serialport::TTYPort::pair()` implementation. TauTerm owns the PTY master and exposes only the slave device path to external applications.

The Unix PTY bridge is a byte-stream bridge, not a hardware UART emulator. TX/RX byte transport works, but baud rate, modem control lines, electrical RS-232/485 behavior and third-party serial-port enumeration are not guaranteed to behave like physical hardware.

No `socat`, Homebrew package, shell `PATH`, `/tmp` symlink or external helper process is required for the Unix virtual serial bridge.

## Signing status

Tauri updater artifacts remain cryptographically signed and verified by the release pipeline. Every release manifest must contain these five updater targets:

- `windows-x86_64-nsis`
- `linux-x86_64-deb`
- `linux-x86_64-rpm`
- `linux-x86_64-appimage`
- `darwin-aarch64-app`

Operating-system distribution signing is not yet enabled:

- Windows Authenticode: not yet enabled; SmartScreen may warn on first install.
- macOS Developer ID/notarization: not yet enabled; Gatekeeper may require a one-time right-click → Open.

macOS remains a tech preview until signing/notarization and clean-machine validation are in place.

## TFTP exposure

TauTerm keeps the existing TFTP defaults for compatibility. Binding to all interfaces while remote writes and overwrite are enabled exposes a writable TFTP service to the reachable network and requires explicit user confirmation before startup. Use this configuration only on trusted networks. On Linux, UDP port 69 is a privileged port and a normal user may need to choose a non-privileged port instead of running the entire application as root.
