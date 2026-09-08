# Building TauTerm / 构建 TauTerm

This is the community source of truth for local development prerequisites and source builds. Executable npm command definitions live in the root `package.json`; this document explains when to use them.

本文是源码开发环境与构建前置条件的社区权威文档。可执行的 npm 命令以根目录 `package.json` 为准。

## Requirements / 环境要求

| Component | Requirement |
|---|---|
| Node.js | 22.x |
| Rust | Exact stable toolchain pinned by `rust-toolchain.toml`, including rustfmt and Clippy |
| npm | Version bundled with the supported Node.js runtime |
| CMake | 3.20+ for the vendored TRDP native helper |
| Native compiler | MSVC on Windows; system C/C++ toolchain on Linux/macOS |
| Tauri system libraries | Platform-specific, as required below |

Run `npm run toolchain:check` after setting up Rust.

## Windows

Install:

- Node.js 22;
- rustup, then let `rust-toolchain.toml` select the repository toolchain;
- Visual Studio Build Tools with Desktop development with C++ and a current Windows SDK;
- CMake 3.20+.

NSIS is needed only when building the Windows installer.

The normal development command automatically builds the vendored TCNOpen/TRDP native helper before starting the desktop app. A separate TCNOpen SDK is not required.

Windows virtual serial support uses the bundled com0com resources. Maintenance details for that third-party driver are intentionally kept only in [the com0com skill](../../.agents/skills/tauterm-com0com/SKILL.md).

## Linux

Install Node.js 22, rustup, CMake, a native compiler, and the development packages required by Tauri/WebKitGTK. For Ubuntu/Debian the typical baseline includes:

```bash
sudo apt update
sudo apt install -y \
  libwebkit2gtk-4.1-dev \
  libappindicator3-dev \
  librsvg2-dev \
  patchelf \
  libssl-dev \
  libgtk-3-dev \
  libsoup-3.0-dev \
  libjavascriptcoregtk-4.1-dev \
  libudev-dev \
  cmake \
  build-essential
```

TRDP live Monitor dynamically loads the system libpcap at runtime. Offline pcap/pcapng analysis does not require live-capture support.

## macOS

Install Xcode Command Line Tools, Node.js 22, rustup, and CMake.

TRDP live Monitor uses the system libpcap at runtime. The virtual serial bridge is implemented with an in-process POSIX PTY and does not require an external bridge process.

## Development

```bash
npm install
npm run tauri dev
```

`npm run tauri dev` is the complete desktop development entry and prepares the TRDP native helper before Vite/Tauri starts. Use `npm run dev` only when you explicitly need the frontend Vite server without the desktop backend.

For isolated TRDP native-build diagnosis:

```bash
npm run trdp:build
```

## Validation

The repository CI baseline includes documentation checks, TypeScript checking/building, Rust formatting, strict Clippy, Rust tests, protocol-fixture self-tests, blocking-command/runtime-error contracts, and a separate real TauTerm WebDriver smoke workflow.

Useful local checks:

```bash
npm run docs:check
npm run license:check
npm run check:tauri-blocking
npm run check:runtime-errors
npm run license:cargo
npx tsc --noEmit
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --locked --all-targets --no-deps --manifest-path src-tauri/Cargo.toml -- -D warnings
cargo test --locked --manifest-path src-tauri/Cargo.toml
```

Additional focused checks are defined in `package.json`. Runtime E2E, scheduled performance contracts, soak tests and dependency advisory checks are documented in [TESTING.md](TESTING.md). The maintainer-oriented command table and daily workflow are kept in [docs/maintainer/DEVELOPMENT.md](../maintainer/DEVELOPMENT.md).

## Production packages

Use the repository scripts/workflows rather than manually reconstructing packaging steps. The exact currently supported artifact matrix is maintained in [SUPPORTED_PLATFORMS.md](SUPPORTED_PLATFORMS.md).

Release builds have additional signing, staging, and artifact verification requirements. Maintainers should follow [RELEASING.md](RELEASING.md).
