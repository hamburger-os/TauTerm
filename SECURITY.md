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

## Scope notes

TauTerm handles credentials, remote hosts, serial devices, network traffic, local files, and application updates. Reports involving credential exposure, command execution, path traversal, unsafe file transfer, updater integrity, or privilege boundary violations are especially important.
