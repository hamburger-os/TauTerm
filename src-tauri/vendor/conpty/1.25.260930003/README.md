# ConPTY engine redistributable

TauTerm pins the Microsoft Windows Console ConPTY engine for its Local Shell
sessions. The engine implementation is the same one Windows Terminal uses; the
in-box `conhost` engine drops mouse-tracking modes that VT-first programs write
before switching the console to raw input, which made full-screen TUIs (pi and
other Node/Bun/Deno/Python programs) unresponsive to wheel and click input.

This directory contains the unmodified redistributable payload from the official
NuGet package. Do not patch these binaries; upgrade by replacing the versioned
directory and updating both `THIRD_PARTY_LICENSES.md` and
`src-tauri/src/plugins/local_shell/conpty_engine.rs`.

## Provenance

| Field | Value |
|---|---|
| Package | `Microsoft.Windows.Console.ConPTY` |
| Version | `1.25.260930003` (stable) |
| Source | `https://api.nuget.org/v3-flatcontainer/microsoft.windows.console.conpty/1.25.260930003/microsoft.windows.console.conpty.1.25.260930003.nupkg` |
| Project | `https://github.com/microsoft/terminal` |
| License | MIT (`LICENSE` in this directory) |
| Supported OS | Windows 10.0.17763 and above |

## Files (`win-x64/`)

| File | Origin inside the package | SHA-256 |
|---|---|---|
| `conpty.dll` | `runtimes/win-x64/native/conpty.dll` | `FEEEF341D891643C62D30B6B07800BC70F0BC148F44CB8C3BEE84AA557AE805A` |
| `OpenConsole.exe` | `build/native/runtimes/x64/OpenConsole.exe` | `3D66B23D0A71BB8EED2B77EDC8B9DF9BF54CE6C8FB74C863A30E760997F80586` |

`OpenConsole.exe` must stay in the same directory as `conpty.dll`: the DLL
launches its sibling console host.

Only the `win-x64` payload is vendored because the Windows release target is
x64. An arm64 build would look for `conpty/win-arm64/` and fall back to the
in-box engine (with a logged warning) until that payload is added.

## How TauTerm uses it

`conpty_engine::install()` preloads `conpty.dll` by absolute path on the first
Local Shell spawn. `portable-pty` opens `conpty.dll` by base name afterwards, and
Windows resolves that to the already loaded image, so the pinned engine owns
every pseudo console in the process. The Windows bundle installs these files to
`<install dir>/conpty/win-x64/`; development builds read this vendor directory.
`npm run check:conpty` verifies the files, hashes and bundle mapping.
