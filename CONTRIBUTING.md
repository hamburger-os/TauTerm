# Contributing to TauTerm

TauTerm is under active development. Contributions are welcome!

## Development Environment Setup

See the [Build & Run section in README.md](README.md#构建与运行) for platform-specific setup instructions.

Quick start:

```bash
git clone https://github.com/hamburger-os/TauTerm.git
cd TauTerm
npm install
npm run tauri dev
```

### Requirements

| Component | Version |
|-----------|---------|
| Node.js | >= 18 |
| Rust | >= 1.75 |
| npm | >= 9 |
| NSIS | >= 3.0 (Windows only, for installer builds) |

## Project Structure

The project follows a microkernel plugin architecture:

- `src-tauri/src/kernel/` — Microkernel modules (plugin host, session store, config store, etc.)
- `src-tauri/src/channel/` — I/O abstraction layer (`Channel` / `AsyncChannel` traits)
- `src-tauri/src/transfer/` — File transfer subsystem (three strategies)
- `src-tauri/src/plugins/` — Built-in protocol plugins (Serial, SSH)
- `src-tauri/src/virtual_port/` — Virtual serial port bridge (com0com on Windows, socat on Linux)
- `src-tauri/src/security/` — Credential store (keyring + AES-256-GCM)
- `src/` — React frontend (TypeScript)
  - `src/core/` — Frontend kernel API (plugin registry, tab host, event bus)
  - `src/components/` — UI components
  - `src/plugins/` — Plugin frontend registrations
  - `src/styles/` — Global CSS tokens and theme definitions

## How to Add a New Protocol Plugin

1. Create a plugin directory under `src-tauri/src/plugins/`
2. Implement the `ProtocolAdapter` trait (see [Backend Core Traits in README.md](README.md#后端核心-trait))
3. Write a `manifest.json` declaring metadata and capabilities
4. Register frontend components via `registerPlugin()` in `src/plugins/`
5. Register the plugin in Plugin Host (`src-tauri/src/lib.rs` → `plugin_host.register_plugin()`)
6. Add protocol routing in `commands.rs` → `connect_session` match branch

A detailed plugin SDK guide will be available with the v1.0 release.

## Theme Development

All UI components follow the **Liquid Glass v3** design system. When creating or modifying components:

- Use CSS custom properties from `src/styles/tokens.css` — never hardcode colors
- Test across all three themes: Google Glow (dark), Obsidian (dark), Frosted (light)
- Reference the `tauterm-theme` skill (`.claude/skills/tauterm-theme/SKILL.md`) for detailed rules

## Pull Request Process

1. Fork the repository and create a feature branch
2. Make your changes, following existing code style
3. Verify your changes:
   - `npx tsc --noEmit` — TypeScript type check
   - `cargo clippy --no-deps --manifest-path src-tauri/Cargo.toml` — Rust lint
   - `cargo test --manifest-path src-tauri/Cargo.toml` — Rust tests
   - `npm run tauri dev` — Manual smoke test
4. Open a pull request with a clear description of your changes

## Code Style

- **Rust**: Follow standard Rust conventions (`rustfmt`). Use `thiserror` for error types. Prefer `#[cfg(target_os = "...")]` for platform-specific code.
- **TypeScript/React**: Use functional components with hooks. CSS Modules for component styles. TypeScript strict mode is enabled.
- **Commits**: Write descriptive commit messages in English or Chinese. No strict conventional commits format required.

## License

By contributing, you agree that your contributions will be licensed under the MIT License.
