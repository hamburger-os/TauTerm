# Contributing to TauTerm

Thanks for contributing to TauTerm. This guide is the community entry point; implementation rules for AI agents live in [AGENTS.md](AGENTS.md).

## Before you start

- For bugs and feature work, search existing issues and pull requests first.
- For security vulnerabilities, use [SECURITY.md](SECURITY.md) instead of a public issue.
- Keep changes focused. Large architecture changes are easier to review when the design boundary is clear before implementation.

## Development setup

Use the authoritative platform instructions in [docs/community/BUILDING.md](docs/community/BUILDING.md).

Quick start:

```bash
git clone https://github.com/hamburger-os/TauTerm.git
cd TauTerm
npm install
npm run tauri dev
```

The exact Rust version is pinned in `rust-toolchain.toml`. Executable npm commands are defined in `package.json`; documentation should link to those commands rather than maintain a second exhaustive command catalog.

## Architecture and documentation

TauTerm separates common Session/Workspace/platform mechanisms from protocol-specific behavior. The maintainer-facing architecture index is [docs/README.md](docs/README.md).

If your change affects architecture, module responsibility, persistent state, security boundaries, platform behavior, or a user-visible workflow, update the matching document under [docs/modules/](docs/modules/) in the same pull request. Protocol/platform/security work should also consult the relevant authority index under [docs/knowledge/](docs/knowledge/).

Documentation follows a single-source-of-truth policy. Do not create a second protocol matrix, roadmap, release-note file, theme specification, or build-command table when an existing canonical document owns that information.

For documentation maintenance details, see [`.agents/skills/tauterm-docs/SKILL.md`](.agents/skills/tauterm-docs/SKILL.md).

## Code style

- **Rust:** use standard Rust conventions and `rustfmt`; strict Clippy warnings are enforced.
- **TypeScript/React:** TypeScript strict mode is enabled; prefer existing component and context patterns.
- **Platform code:** keep operating-system differences behind explicit platform boundaries.
- **UI text:** keep English and Chinese i18n key sets aligned.
- **Themes:** follow [`.agents/skills/tauterm-theme/SKILL.md`](.agents/skills/tauterm-theme/SKILL.md); do not hard-code a second theme contract in component documentation.

## Validation

Run the checks relevant to your change. The normal baseline is:

```bash
npm run docs:check
npx tsc --noEmit
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --locked --all-targets --no-deps --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --locked --manifest-path src-tauri/Cargo.toml
```

Use `npm run tauri dev` for a desktop smoke test when the change affects runtime behavior. Protocol- or platform-specific checks are documented beside their owning implementation or skill.

## Pull requests

A good pull request:

1. explains the problem and intended behavior;
2. keeps unrelated refactors out of the diff;
3. includes tests or a clear validation path where practical;
4. updates the canonical documentation when a documented contract changes;
5. passes CI without suppressing warnings.

Commit messages may be English or Chinese. No specific conventional-commit format is required.

## License

By contributing, you agree that your contribution is licensed under **MIT OR Apache-2.0**, at the recipient's option.
