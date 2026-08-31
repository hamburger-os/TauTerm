# TauTerm v0.5.1

TauTerm v0.5.1 is a minimal patch release created specifically to validate the signed online update flow from the official v0.5.0 release.

## What changed
- Application version bumped from v0.5.0 to v0.5.1.
- No intentional user-facing product changes.

## Online update validation
This release is intended to validate:
- v0.5.0 recognizes v0.5.1 as available.
- `latest.json` resolves the correct platform artifact.
- Tauri verifies the updater signature.
- The update downloads and installs successfully.
- After restart/relaunch, TauTerm reports v0.5.1.
- Existing settings and saved connections remain available.

## Distribution
- Windows x64: NSIS installer and signed updater package
- Linux x64: DEB, RPM, and AppImage plus updater signatures
- macOS Apple Silicon: DMG and signed app updater archive

This release intentionally contains no unrelated product changes so updater behavior can be evaluated in isolation.
