# TauTerm v0.5.0 — Networking & least-privilege security

TauTerm v0.5.0 expands the project from an SSH/SFTP + serial workspace into a broader network-debugging tool, while also changing the Windows privilege model so the main application can run as a normal user.

## Why this release matters

This release is aimed at engineers who move between servers, embedded devices and ad-hoc network debugging. Instead of opening separate tools for SSH/SFTP, serial I/O, TCP/UDP testing, Telnet, throughput tests and remote logs, TauTerm now covers those workflows inside the same session-oriented desktop application.

On Windows, TauTerm no longer needs to run fully elevated just to support the optional com0com virtual serial bridge. Privileged driver and virtual-port operations are delegated to a narrow LocalSystem service, while the main UI process runs as a standard user. On Linux and macOS, virtual serial endpoints now use an in-process native PTY bridge, so no `socat` installation or external helper process is required.

## Highlights

- **TCP/UDP Network Debug** — TCP client/server and UDP client/server workflows, including multi-client TCP sessions, UDP unicast/broadcast/multicast, TEXT/HEX views, peer statistics and target-aware sending.
- **Telnet** — RFC 854 negotiation, NAWS window-size handling, keepalive and local-echo synchronization.
- **iPerf2 + iPerf3** — client and server bandwidth testing with TCP/UDP modes, reverse/bidirectional options where supported, bandwidth caps, parallel streams and result history.
- **Remote journald viewer over SSH** — live streaming, history queries, cursor-based pagination and JSON export.
- **Session-level character-set transcoding** — UTF-8, GBK, GB18030, Big5, Shift-JIS, EUC-JP, EUC-KR and ISO-8859-1/windows-1252 workflows without changing raw HEX paths.
- **Least-privilege Windows virtual serial support** — the main app runs as a normal user; privileged com0com operations are delegated to `TauTermService`, with fallback to on-demand UAC when needed.
- **Native Unix virtual serial bridge** — Linux and macOS use an in-process POSIX PTY bridge with no `socat`, shell `PATH`, `/tmp` symlink or helper process dependency.
- **Credential storage hardening** — TauTerm prefers the operating-system keyring and falls back to an Argon2id-derived AES-256-GCM vault that is unlocked for the current app session when native storage is unavailable.
- **SFTP usability improvements** — list/grid views, inline toolbar actions and shared file-category icons.
- **Safer TFTP exposure** — starting a remotely writable TFTP server on a non-loopback interface with overwrite enabled now requires explicit confirmation.
- **Release/update hardening** — release builds validate the exact updater target set, verify updater signatures and public artifacts before promotion, and fail closed if final validation does not pass.

## Install

Download the assets attached to the GitHub Release.

- **Windows 10/11 x86_64:** NSIS `.exe` installer.
- **Linux x86_64:** `.deb`, `.rpm` or `.AppImage` packages built against an Ubuntu 22.04 baseline.
- **macOS Apple Silicon (`aarch64`):** `.dmg` plus the signed updater app archive; this target remains a tech preview.

Windows ARM64, Linux ARM64, macOS Intel and Windows MSI are not release targets for v0.5.0.

The macOS build is not yet Developer ID signed or notarized, so Gatekeeper may require a one-time right-click → **Open**.

The optional virtual serial bridge uses:

- **Windows:** the bundled open-source **com0com** driver, with privileged operations handled by `TauTermService`.
- **Linux/macOS:** an in-process POSIX PTY bridge owned by TauTerm; no external `socat` dependency is required.

## What changed for Windows users

Earlier TauTerm builds could require broad administrator elevation for virtual COM operations. v0.5.0 changes that model:

1. `tauterm.exe` runs as the interactive user.
2. A small background service performs only the privileged com0com operations TauTerm needs.
3. The service verifies the connecting client before accepting requests.
4. If the service is unavailable, TauTerm can fall back to the previous on-demand UAC path.

This keeps normal SSH, SFTP, serial and network-debugging work out of an elevated desktop process.

## Release integrity

The v0.5.0 release pipeline runs the normal quality gate before packaging, builds the supported Windows, Linux and Apple Silicon macOS targets, verifies updater signatures, generates `latest.json` and `SHA256SUMS`, and checks the uploaded asset set before a stable release can become the GitHub `latest` updater target.

Updater artifacts are cryptographically signed. Operating-system distribution signing is separate: Windows Authenticode and macOS Developer ID/notarization are not yet enabled.

## What I would like feedback on

If you try this release, the most useful feedback is around:

- TCP/UDP debugging behavior on real devices, especially broadcast and multicast;
- Windows com0com installation, upgrades and uninstall cleanup;
- Linux/macOS packaging and first-launch experience;
- native PTY virtual-serial interoperability on Linux/macOS;
- device encodings such as GBK/GB18030 and Shift-JIS;
- the one feature in your current terminal / serial / network tool that still prevents you from switching to TauTerm.

Please open a GitHub issue with reproducible steps when reporting a bug.

## Known limitations / roadmap

TauTerm is still pre-1.0. Local shell, SSH tunnels / jump hosts, session groups, FTP, recording, split panes and a more formal plugin SDK remain roadmap items.

For the complete implementation-level change list, see [`CHANGELOG.md`](../CHANGELOG.md).
