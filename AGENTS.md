# TauTerm Agent Guide

This file is the repository-level contract for AI coding agents. It is intentionally tool-neutral and is the canonical entry point for Codex, TRAE, and other agents that support `AGENTS.md`.

## Instruction order

Apply instructions in this order:

1. the user's current request;
2. the nearest applicable `AGENTS.md` when nested files exist;
3. this root `AGENTS.md`;
4. task-specific skills under `.agents/skills/`.

Do not create tool-specific copies of these rules. A tool adapter may point here, but it must not become a second source of truth.

## Project

TauTerm is a local-first desktop engineering workbench built with Tauri v2, Rust, React, and TypeScript. It brings terminal, device, and network-debugging workflows into one session-oriented workspace.

The repository default branch is `master`.

## Documentation layers

Documentation is deliberately split by audience.

| Layer | Canonical location | Language | Purpose |
|---|---|---|---|
| AI | `AGENTS.md`, `.agents/skills/` | English preferred | Agent rules, task procedures, machine-oriented engineering contracts |
| Community | `README.md`, `README.zh-CN.md`, `CONTRIBUTING.md`, `docs/community/` | English and/or Chinese | Public project entry, contributing, build, platform, and release procedures |
| Maintainer | `docs/README.md`, `docs/modules/`, `docs/maintainer/`, `docs/product/` | Chinese | Architecture, solution review, maintainer operations, and product direction |
| Knowledge | `docs/knowledge/` | Chinese index + authoritative external sources | Standards, protocol specifications, platform documentation, upstream behavior, and compliance references |

### Single-source-of-truth rules

A durable fact must have one owner. Other documents link to it instead of restating it.

- Public project positioning and current high-level capability summary: root `README.md` / its Chinese mirror.
- Released and unreleased change history: `CHANGELOG.md`.
- Current architecture and module-level design: `docs/README.md` and exactly one owner document under `docs/modules/`.
- Future product direction and commercial/hardware strategy: `docs/product/`.
- Build, platform, and release procedures for contributors: `docs/community/`.
- Maintainer daily workflow and command navigation: `docs/maintainer/DEVELOPMENT.md`; executable npm definitions remain in `package.json`.
- External standards and upstream authoritative references: `docs/knowledge/`; do not copy paid/copyrighted standards into the repository.
- Visual theme implementation specification: `.agents/skills/tauterm-theme/SKILL.md`.
- Executable npm command definitions: `package.json`.
- Security reporting policy: `SECURITY.md`.
- Third-party licensing inventory: `THIRD_PARTY_LICENSES.md`.

Do not duplicate command tables, protocol matrices, architecture descriptions, roadmap status, release notes, or theme specifications across documents.

## Code and documentation are one change

A change is incomplete when it changes a documented contract but leaves its owner document stale.

Update the matching maintainer document in the same task when changing architecture, module responsibilities, persistent data, session lifecycle, security boundaries, platform behavior, or a user-visible workflow. Prefer updating the design document before or alongside implementation so the document states the intended result.

The maintainer reviews architecture and solution design primarily through `docs/README.md` and `docs/modules/`. These documents are therefore a code-review proxy: they must be accurate enough to reveal a meaningful design change, but they should not reproduce functions, structs, line-by-line control flow, or implementation trivia.

User-visible additions or removals may also require the root README. Release history belongs only in `CHANGELOG.md`.

## Maintainer module ownership

Use this map to decide which design document must change.

| Code / concern | Owner document |
|---|---|
| SessionStore, plugin/channel contracts, common connection routing | `docs/modules/CORE.md` |
| application shell, Settings, renderers, i18n, shortcuts and shared frontend structure | `docs/modules/UI_FOUNDATION.md` |
| Pane/Workspace layout and restoration | `docs/modules/WORKSPACE.md` |
| Serial and virtual serial integration | `docs/modules/SERIAL.md` |
| shared file transfer abstraction, X/Y/ZModem and SFTP orchestration | `docs/modules/TRANSFER.md` |
| SSH, remote terminal/channel model, file-manager integration and journald | `docs/modules/SSH.md` |
| Local Shell, PTY/ConPTY, per-child elevation | `docs/modules/LOCAL_SHELL.md` |
| TCP/UDP Network Debug, TFTP, Telnet, iperf | `docs/modules/NETWORK.md` |
| TRDP Node/Monitor, capture, XML/Dataset, native sidecar | `docs/modules/TRDP.md` |
| SendBar, auto-reply, scripting and communication automation | `docs/modules/AUTOMATION.md` |
| data batching, logging, statistics and stateless engineering tools | `docs/modules/OBSERVABILITY_TOOLS.md` |
| credential storage, privileged helpers, packaging/updater trust boundaries | `docs/modules/PLATFORM_SECURITY.md` |

If a change spans modules, update each affected owner document, but keep each fact in the document that owns it.

## Working rules

- Inspect the smallest relevant code and documentation surface first.
- Before changing protocol semantics, terminal/platform behavior, security boundaries, or licensing, read the matching `docs/knowledge/*.md` authority index and verify the relevant primary source.
- Follow existing architecture unless the task intentionally changes it.
- Keep protocol-specific behavior inside protocol modules; shared lifecycle and platform concerns belong in the common core.
- Preserve local-first operation and least-privilege boundaries.
- Do not add compatibility layers for obsolete internal designs unless the user explicitly asks for them.
- Do not invent performance claims, compatibility claims, test results, screenshots, or release status.
- User-facing brand/copy must remain neutral and must not borrow other companies' product identities.
- UI text changes must keep `src/i18n/locales/en-US.json` and `zh-CN.json` keys aligned.
- Theme changes must follow `.agents/skills/tauterm-theme/SKILL.md`; do not restate the theme spec elsewhere.

## Documentation procedure

For any documentation-related change, read `.agents/skills/tauterm-docs/SKILL.md`.

After changing documentation, run:

```bash
npm run docs:check
```

When third-party source, bundled binaries, licenses, or dependency licensing changes, also run `npm run license:check` and `npm run license:cargo`.

The documentation check is a required CI gate.

## Validation

Choose checks that match the change. The normal baseline is:

```bash
npm run docs:check
npx tsc --noEmit
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --locked --all-targets --no-deps --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --locked --manifest-path src-tauri/Cargo.toml
```

Use `npm run tauri dev` for end-to-end desktop smoke testing when behavior requires it. Package and release commands are defined in `package.json` and documented in `docs/community/`; do not copy them into new rule files.
