# Supported Platforms / 支持平台

This file is the canonical repository document for current packaging targets and important platform-specific runtime requirements.

本文是当前打包目标与平台运行时差异的权威文档。

## Current release targets

| Platform | Architecture | Packages | Status |
|---|---|---|---|
| Windows | x86_64 | NSIS installer | Supported release target |
| Linux | x86_64 | `.deb`, `.rpm`, `.AppImage` | Supported release target |
| macOS | Apple Silicon / aarch64 | `.dmg` + updater app archive | Tech preview release target |

Windows ARM64, macOS Intel, and other architectures are not current packaged release targets unless the release workflow changes.

## Signing and updater status

TauTerm's Tauri updater artifacts are signed and verified by the release pipeline. Platform-native distribution signing/notarization is separate from updater signing.

- Tauri updater signing is mandatory for published updater artifacts and is verified by the release pipeline.
- Windows Authenticode publisher signing is **not enabled in the current development-stage release process**. Current NSIS packages and TauTerm-owned Windows binaries may therefore be unsigned at the publisher/Authenticode layer and can trigger Windows reputation/SmartScreen warnings. Authenticode + RFC 3161 timestamping remains a planned release-trust milestone before TauTerm is treated as a fully production-distributed Windows desktop product.
- macOS remains a tech preview and may require a one-time manual open when the build is not notarized.
- Linux packages use the Ubuntu 22.04 release environment as the current build baseline.

The release workflow is the authority for the actual artifact set.

## TRDP

TRDP Node uses the bundled TauTerm native sidecar built from vendored TCNOpen source.

Live Monitor capture additionally depends on platform capture support:

- **Windows:** user-installed Npcap; TauTerm does not redistribute it.
- **Linux/macOS:** system libpcap.

Offline pcap/pcapng analysis requires neither Npcap nor a live capture interface.

Architecture and safety boundaries are documented in [the TRDP module design](../modules/TRDP.md).

## Virtual serial

- **Windows:** com0com-backed virtual COM pairs, with privileged operations delegated through the platform privilege boundary.
- **Linux/macOS:** in-process POSIX PTY bridge; no external `socat` helper is required.

The virtual bridge transports bytes; it is not a hardware UART/electrical simulator.

## Build requirements

Source-build prerequisites are maintained in [BUILDING.md](BUILDING.md). Packaging details are intentionally not duplicated here.
