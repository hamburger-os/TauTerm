# Third-party licenses

TauTerm itself remains licensed under **MIT OR Apache-2.0**. The components below are separate third-party works and remain under their respective licenses.

## TCNOpen TRDP 3.0.0.0

TauTerm vendors the source files required by its TRDP integration under:

`src-tauri/vendor/tcnopen/`

Those files are a snapshot of **TCNOpen TRDP 3.0.0.0** and retain their upstream copyright and **Mozilla Public License 2.0 (MPL-2.0)** notices. TauTerm does not relicense or remove notices from those files.

Normal TauTerm builds do **not** download TCNOpen and do not require a separately installed TCNOpen SDK. TauTerm's native build wrapper compiles the vendored PD/MD + VOS source subset together with the separate TauTerm-owned helper executable `tauterm-trdp-bridge`.

Source provenance is recorded in `src-tauri/vendor/tcnopen/SOURCE.json` and `src-tauri/vendor/tcnopen/README.md`. Maintainers can compare or refresh the committed snapshot against the official TCNOpen 3.0.0.0 SourceForge release with `scripts/vendor_tcnopen.py`. That maintenance operation is intentionally separate from normal/offline builds.

When redistributing a build containing the TCNOpen-covered executable code, distributors must preserve the applicable notices and comply with the MPL-2.0 requirements for the MPL-covered TCNOpen files. The corresponding MPL source is already included in the TauTerm source repository under `src-tauri/vendor/tcnopen/`.

TauTerm-owned bridge code, CMake build files, Rust code, and TypeScript code remain **MIT OR Apache-2.0** and are kept outside the MPL-covered vendor directory.

TCNOpen project: https://sourceforge.net/projects/tcnopen/
MPL-2.0: https://www.mozilla.org/MPL/2.0/

**TRDPSpy is not used, linked, vendored, or distributed by TauTerm.** Its GPL-licensed Wireshark plugin is deliberately outside this integration.

## Npcap / libpcap

TauTerm does **not** redistribute Npcap. On Windows, live TRDP Monitor capture dynamically loads an Npcap installation already present on the user's machine (`wpcap.dll`). Offline `.pcap` / `.pcapng` analysis does not require Npcap.

On Linux/macOS, the native helper dynamically loads the system libpcap when live capture is requested. TauTerm does not vendor libpcap.
