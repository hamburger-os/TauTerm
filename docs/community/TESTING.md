# Testing TauTerm / 测试 TauTerm

This document is the repository source of truth for automated verification levels, long-running reliability checks, and the remaining role of manual hardware validation.

本文定义 TauTerm 的自动测试分层、性能/长稳验证方式，以及仍需真实硬件人工验证的边界。

## Test layers

| Layer | Scope | Typical trigger | Purpose |
|---|---|---|---|
| Static/contract | TypeScript, Clippy, formatting, docs, plugin/persistence/UI contracts | Every PR | Catch structural regressions cheaply |
| Rust unit/integration | Session/I/O, persistence, security, protocol algorithms and localhost sockets | Every PR | Verify core lifecycle and error semantics |
| Protocol fixtures | Reusable Telnet/Serial/TRDP test peers and fixture self-tests | Every PR where practical | Keep deterministic simulated devices healthy |
| Runtime E2E | Real TauTerm desktop app driven through Tauri WebDriver on Windows/Linux | Every PR | Verify user-visible application paths in an actual WebView |
| Performance contract | Release-mode I/O dispatch and persistence benchmark with JSON artifact | Scheduled/manual | Track performance trends without noisy per-PR hard thresholds |
| Reliability soak | Repeated create/write/shutdown I/O lifecycle for configurable duration | Scheduled/manual | Find leaks, hangs and lifecycle accumulation |
| Dependency security | npm + RustSec advisory reports and Dependabot | Scheduled/manual + update PRs | Keep dependency risk visible |

## Pull-request gate

The normal CI workflow remains the fast correctness gate. In addition to build/type/Rust checks it validates:

- Tauri commands that perform potentially blocking filesystem/process/credential/driver work are not synchronous command handlers;
- frontend ErrorBoundary/unhandled errors use the shared diagnostic path and localized public copy;
- reusable protocol fixture scripts remain executable and the Telnet fixture passes an actual loopback login/command exchange;
- existing split-layout, product-integrity, security and persistence contracts.

Runtime E2E is a separate PR workflow because it builds and starts the real desktop application.

## Runtime E2E

tests/e2e uses WebdriverIO with the Tauri service and the external tauri-driver provider. No WebDriver/automation plugin is shipped in TauTerm itself.

The permanent smoke contract covers:

- application shell starts and renders;
- no root document overflow at the default window size;
- command palette opens and closes;
- new-session workflow opens and closes;
- no unhandled frontend runtime errors occur during those interactions.

Stable data-testid attributes are test contracts only; they do not carry product state.

Windows and Linux are the automated real-WebView targets for this external-driver path. macOS remains covered by build/Rust checks and manual release validation until a no-production-backdoor native automation path is adopted.

## Protocol fixtures

Reusable fixture assets include:

- scripts/test-serial-session.py: high-fidelity RT-Thread/FinSH-style Serial device simulator, including transfer/script scenarios; real virtual COM use remains Windows/manual where com0com is required;
- scripts/test-telnet-server.py: deterministic Telnet negotiation/login/shell peer;
- scripts/test-protocol-fixtures.py: automated fixture self-test;
- tools/trdp-test-peer/: native TRDP interoperability peer used by the TRDP workflow.

Protocol fixtures should model peer behavior, not duplicate TauTerm protocol implementations.

## Fault and lifecycle testing

Core I/O tests deliberately exercise abnormal paths such as partial async writes, injected write failure, cancellation, graceful Shutdown and repeated channel create/write/shutdown cycles.

When a new Session resource owns a thread, task, subprocess, native handle, transfer, listener or VM, tests should cover interruption during an active operation whenever practical.

## Performance contract

Performance results are measurements, not a synthetic TauTerm score.

The current release-mode contract records:

- 32 MiB in-memory Session I/O dispatch throughput;
- 200 atomic 16 KiB persistence replacements;
- elapsed time and derived throughput/rate.

The workflow stores performance-contract.json and logs as artifacts. Hosted-runner measurements are initially trend data, not strict PR blockers. A numerical threshold should become a hard gate only after enough history exists to distinguish product regressions from runner variance.

## Reliability soak

The dedicated reliability workflow runs repeated Session I/O lifecycle iterations for 5 seconds to 8 hours. Scheduled runs use a practical default; maintainers can request longer runs before a release.

The soak produces a JSON artifact containing duration, completed iterations and payload volume. Any hang, panic or incorrect byte accounting fails the run.

## Dependency security

Dependabot tracks npm, Cargo and GitHub Actions dependencies weekly. The Dependency Security workflow also runs:

- production npm advisory gate at high severity;
- full npm advisory report;
- RustSec cargo audit.

Security reports are stored as workflow artifacts for review.

## Diagnostics during testing

Settings → About → Diagnostics can export a sanitized JSON support bundle. It contains build/runtime health, plugin metadata, aggregated Session state and log loss counters.

It excludes credentials, endpoint values, Session names, raw Session payloads, System Log contents and Session Data Log contents. The native save dialog is opened by Rust so the WebView does not receive the destination path. Diagnostics are support evidence, not an Engineering Recording.

## Manual validation that still matters

Automation intentionally does not pretend to replace real environment testing. Before releases that touch the relevant areas, manual/real-hardware validation remains valuable for:

- physical Serial adapters, unplug/replug and vendor drivers;
- Windows com0com/UAC/service behavior;
- real SSH servers, agents/key formats and remote journald/SFTP environments;
- Npcap/libpcap live capture permissions and real TRDP networks;
- OS-specific installer/updater/reputation behavior;
- visual judgment across GPUs, scaling factors and accessibility settings.

The goal is to reduce manual testing to cases where real hardware, OS policy or human visual judgment is genuinely necessary.
