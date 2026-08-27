# TauTerm v0.5.0 — Networking & least-privilege security

TauTerm v0.5.0 expands the project from an SSH/SFTP + serial workspace into a broader network-debugging tool, while also changing the Windows privilege model so the main application can run as a normal user.

## Why this release matters

This release is aimed at engineers who move between servers, embedded devices and ad-hoc network debugging. Instead of opening separate tools for SSH/SFTP, serial I/O, TCP/UDP testing, Telnet, throughput tests and remote logs, TauTerm now covers those workflows inside the same session-oriented desktop application.

On Windows, TauTerm no longer needs to run fully elevated just to support the optional com0com virtual serial bridge. Privileged driver and virtual-port operations are delegated to a narrow LocalSystem service, while the main UI process runs as a standard user.

## Highlights

- **TCP/UDP Network Debug** — TCP client/server and UDP client/server workflows, including multi-client TCP sessions, UDP unicast/broadcast/multicast, TEXT/HEX views, peer statistics and target-aware sending.
- **Telnet** — RFC 854 negotiation, NAWS window-size handling, keepalive and local-echo synchronization.
- **iPerf2 + iPerf3** — client and server bandwidth testing with TCP/UDP modes, reverse/bidirectional options where supported, bandwidth caps, parallel streams and result history.
- **Remote journald viewer over SSH** — live streaming, history queries, cursor-based pagination and JSON export.
- **Session-level character-set transcoding** — UTF-8, GBK, GB18030, Big5, Shift-JIS, EUC-JP, EUC-KR and ISO-8859-1/windows-1252 workflows without changing raw HEX paths.
- **Least-privilege Windows virtual serial support** — the main app runs as a normal user; privileged com0com operations are delegated to `TauTermService`, with fallback to on-demand UAC when needed.
- **SFTP usability improvements** — list/grid views, inline toolbar actions and shared file-category icons.
- **Signed in-app updater baseline** — v0.5.0 establishes the updater baseline for future stable releases. Release publication verifies the updater manifest and the exact public GitHub-hosted artifacts before promoting a release to the stable update channel.
- **Cross-platform packaging work** — Windows, Linux and macOS release jobs are prepared in GitHub Actions, including Apple Silicon and Intel macOS builds.

## Install

Download the assets attached to the GitHub Release.

- **Windows:** NSIS `.exe` installer.
- **Linux:** `.deb`, `.rpm` or `.AppImage`.
- **macOS:** `.dmg` / `.app` for Apple Silicon or Intel.

The macOS build is still a tech preview and is not notarized, so Gatekeeper may require a one-time right-click → **Open**.

The optional virtual serial bridge uses:

- Windows: bundled open-source **com0com** driver.
- Linux/macOS: **socat**.

## What changed for Windows users

Earlier TauTerm builds could require broad administrator elevation for virtual COM operations. v0.5.0 changes that model:

1. `tauterm.exe` runs as the interactive user.
2. A small background service performs only the privileged com0com operations TauTerm needs.
3. The service verifies the connecting client before accepting requests.
4. If the service is unavailable, TauTerm can fall back to the previous on-demand UAC path.

This keeps normal SSH, SFTP, serial and network-debugging work out of an elevated desktop process.

## What I would like feedback on

If you try this release, the most useful feedback is around:

- TCP/UDP debugging behavior on real devices, especially broadcast and multicast;
- Windows com0com installation, upgrades and uninstall cleanup;
- Linux/macOS packaging and first-launch experience;
- device encodings such as GBK/GB18030 and Shift-JIS;
- the one feature in your current terminal / serial / network tool that still prevents you from switching to TauTerm.

Please open a GitHub issue with reproducible steps when reporting a bug.

## Known limitations / roadmap

TauTerm is still pre-1.0. Local shell, SSH tunnels / jump hosts, session groups, FTP, recording, split panes and a more formal plugin SDK remain roadmap items.

For the complete implementation-level change list, see [`CHANGELOG.md`](../CHANGELOG.md).
