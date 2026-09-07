# Security Policy

## Supported versions

Security fixes are developed against the current `master` branch and are released through the latest supported stable TauTerm version.

## Reporting a vulnerability

Please do **not** open a public issue for a suspected security vulnerability.

Use GitHub's private vulnerability reporting / Security Advisory flow when the repository exposes it, so the report, reproduction details, affected versions, and any proof-of-concept material remain private while the issue is triaged. If no private-reporting entry is available, do not post vulnerability details publicly: use a private contact method published by the repository owner; if none is available, open only a minimal issue asking for a private contact channel and include no vulnerability details.

Include, when possible:

- affected TauTerm version or commit;
- operating system and architecture;
- the feature or trust boundary involved;
- clear reproduction steps;
- impact and prerequisites;
- logs or captures with credentials and private data removed.

## Security architecture

This file owns only the vulnerability-reporting policy. TauTerm's current credential-storage, privilege-separation, native-helper, packaging, and updater trust boundaries are documented in [docs/modules/PLATFORM_SECURITY.md](docs/modules/PLATFORM_SECURITY.md).

Authoritative external security/platform references used during implementation are indexed in [docs/knowledge/PLATFORM_SECURITY.md](docs/knowledge/PLATFORM_SECURITY.md).

## Scope notes

TauTerm is an engineering tool that can open terminals, network listeners, local processes, device interfaces, and protocol analyzers. A report should distinguish a product vulnerability from intentionally powerful behavior that requires the user to explicitly configure and start an operation.

Never include real passwords, private keys, production tokens, or sensitive packet captures in a public discussion.
