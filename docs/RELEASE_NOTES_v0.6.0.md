# TauTerm v0.6.0 — Workspace Foundation

TauTerm v0.6.0 moves the project closer to a daily-driver engineering workbench. This release adds native Local Shell sessions, multi-pane Split View, persistent Workspace context, stronger terminal lifecycle behavior and the credential-storage verification closure prepared after v0.5.1.

## Highlights

### Split View and persistent Workspace context

- Display one to four sessions at the same time with nested split layouts and draggable dividers.
- Keep the selected pane as the active session context while the sidebar shows which saved sessions are placed in panes.
- Restore the last valid pane tree, split ratios, saved-session placement and selected pane after restarting TauTerm.
- Restore context only: sessions come back disconnected and TauTerm never persists or silently recreates live sockets, PTYs, credentials or terminal process state as Workspace data.
- Recover safely from missing/deleted sessions and corrupt, stale or future Workspace payloads without destroying the remaining layout.
- Connect, configure or delete a saved session directly from a disconnected pane.
- Map runtime SSH and Local Shell child channels back to stable parent configurations so durable Workspace placement does not depend on transient runtime channel IDs.

### Native Local Shell

- Run a native PTY-backed local shell on Windows, Linux and macOS inside the same TauTerm session workspace.
- Discover common Windows shells and WSL distributions, or launch a custom executable with explicit arguments and working directory.
- Open multiple independent child terminals from one saved Local Shell configuration.
- On Windows, open an individual native-shell child with `New (as Administrator)` while keeping the main TauTerm process unelevated; elevated children are visibly marked. WSL children are intentionally excluded from this elevation path.

### Terminal lifecycle reliability

- Distinguish user disconnect, remote EOF, I/O failure, device removal and process exit as structured terminal disconnect reasons.
- Preserve the terminal screen and disconnect reason after abnormal disconnects or non-zero Local Shell exits so failures can be inspected.
- Keep startup bytes that arrive before the terminal mounts in a bounded per-session buffer.
- Use separate command and event pipes for elevated Windows Local Shell children so output reads cannot stall input or shutdown.

### Credential-storage trust closure

- Verify credential store/get/list/delete behavior and persistence across store instances on the cross-platform Rust test matrix.
- Require the headless Ubuntu CI leg to exercise the authenticated encrypted fallback instead of silently skipping it.
- Keep the fallback vault in an explicit versioned envelope; unsupported versions fail closed and AEAD tamper-detection remains part of the release gate.
- Prefer the operating-system keyring when available; the fallback uses Argon2id key derivation and AES-256-GCM authenticated encryption with random salt/nonce material and authenticated metadata.

## Upgrade notes

- Users on v0.5.1 can update directly to v0.6.0 through the signed updater path or install the new package normally.
- Workspace restore never auto-connects remote hosts, serial devices, local shells or network-debugging transports after app startup.
- Existing saved session configurations and credential stores keep their existing storage boundaries; Workspace persistence stores only layout/context references.

## Platform notes

- **Windows:** x64 NSIS installer. The installer bundles the separate open-source com0com component for virtual COM bridging. Builds are not yet Authenticode-signed, so SmartScreen may warn on first install.
- **Linux:** x86_64 `.deb`, `.rpm` and `.AppImage`, built on the Ubuntu 22.04 baseline.
- **macOS:** Apple Silicon `.dmg` and updater app archive. macOS remains a tech preview and is not yet notarized, so Gatekeeper may require a one-time right-click → Open.

## Security notes

TauTerm continues to verify SSH host keys, redact sensitive log content, prefer native credential stores and cryptographically verify updater artifacts. Security-sensitive behavior should be reported privately according to `SECURITY.md`.
