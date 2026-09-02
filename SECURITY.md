# Security Policy

## Supported versions

Security fixes are provided for the latest stable TauTerm release. When a vulnerability also affects an upcoming release candidate, the fix should land before that release is published.

## Reporting a vulnerability

Please do **not** open a public GitHub issue for a suspected security vulnerability.

Use GitHub's private vulnerability reporting / Security Advisories for this repository when available. Please include:

- the affected TauTerm version and operating system;
- the affected protocol or feature (for example SSH/SFTP, Serial, TCP/UDP, TFTP, updater);
- reproduction steps or a minimal proof of concept;
- the security impact you observed;
- any mitigation or fix you have already identified.

We will review valid reports, coordinate a fix, and avoid public disclosure until users have a reasonable opportunity to update.

## Credential storage model

TauTerm prefers the operating-system credential store when that backend is available. When no usable native backend is available, TauTerm uses a local authenticated encrypted vault that must be unlocked with a user-supplied master password for the current app session.

The fallback vault uses:

- Argon2id for master-password key derivation;
- AES-256-GCM authenticated encryption;
- random salts and nonces;
- authenticated format/KDF metadata;
- a versioned envelope with fail-closed handling for unsupported versions;
- best-effort zeroization of plaintext/key buffers where practical.

The current `v1` envelope is TauTerm's **first persisted fallback credential format**. There is therefore no older on-disk TauTerm credential format to migrate from. Unsupported future/unknown envelope versions are rejected rather than guessed or silently converted. Corrupt or authentication-failing vault contents are also rejected.

Passwords typed directly into the SSH connection form are not persisted automatically.

The credential-store contract is covered by the normal cross-platform Rust test matrix. It verifies store/get/list/delete behavior and persistence across store instances; the headless Ubuntu GitHub Actions leg is additionally required to exercise the encrypted fallback path. The fallback cryptographic unit test also verifies AEAD tamper detection. These tests are release gates, not a substitute for independent cryptographic review.

## Scope notes

TauTerm handles credentials, remote hosts, serial devices, network traffic, local files, and application updates. Reports involving credential exposure, command execution, path traversal, unsafe file transfer, updater integrity, or privilege boundary violations are especially important.
