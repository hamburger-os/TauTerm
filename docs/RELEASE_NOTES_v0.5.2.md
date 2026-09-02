# TauTerm v0.5.2 — Trust & Hygiene

TauTerm v0.5.2 closes the remaining credential-storage verification gap before the next Foundation work, while also shipping the Local Shell work that landed on `master` after v0.5.1.

## Highlights

### Native Local Shell

- Run a native PTY-backed local shell on Windows, Linux and macOS.
- Discover common Windows shells and WSL distributions, or launch a custom executable with explicit arguments and working directory.
- Open multiple independent terminals from one saved Local Shell configuration.
- On Windows, open an individual native-shell child as Administrator without elevating the main TauTerm process; elevated children are visibly marked.

### Credential-storage trust closure

- The cross-platform Rust test matrix now verifies credential store/get/list/delete behavior and persistence across store instances.
- The headless Ubuntu GitHub Actions leg is required to exercise the authenticated encrypted fallback rather than silently skipping it.
- The fallback vault has an explicit versioned envelope; unsupported versions fail closed.
- Existing AEAD tamper-detection coverage remains part of the release gate.
- `SECURITY.md` now documents the credential-storage trust boundary, fallback construction and test evidence.

The current fallback envelope is `v1`, TauTerm's first persisted credential-vault format. There is no older TauTerm on-disk credential format to migrate. Unknown future/unsupported versions are rejected rather than guessed or silently converted.

### Terminal lifecycle reliability

- Terminal disconnects now distinguish user disconnect, remote EOF, I/O failure, device removal and process exit.
- Abnormal disconnects and non-zero Local Shell exits retain the terminal screen and reason for inspection; normal exits clear it.
- Startup bytes that arrive before the terminal mounts are retained in a bounded buffer.
- Windows elevated Local Shell command/event traffic uses separate local named pipes so output reads do not stall input or shutdown.

### Release hygiene

- v0.5.2 is intended to leave `master` with a closed credential-storage security issue, explicit security documentation and repeatable cross-platform evidence before split-pane/Workspace work begins.
- TFTP path-containment and exposure-confirmation hardening was already completed before this release and remains covered by the existing v0.5.x security posture.

## Security notes

TauTerm prefers the operating-system keyring when available. If a usable native backend is unavailable, the fallback vault uses Argon2id key derivation and AES-256-GCM authenticated encryption with random salt/nonce material and authenticated metadata. Passwords entered directly in the SSH connection form are not persisted automatically.

As with previous releases, security-sensitive behavior should be reported privately according to `SECURITY.md`.
