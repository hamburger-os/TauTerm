# Third-Party Software and License Notices

TauTerm-owned source code is licensed under **MIT OR Apache-2.0**. See [LICENSE](LICENSE) and [LICENSE-APACHE](LICENSE-APACHE).

This file is the repository-level inventory for third-party components that TauTerm **redistributes separately, vendors/modifies in source form, or embeds as a material runtime**. Package-managed Rust/npm dependencies remain traceable through `src-tauri/Cargo.lock` and `package-lock.json`; their upstream licenses are not duplicated line-by-line here.

## Redistributed or vendored components

### com0com 3.0.0.0

- **Purpose:** Windows virtual serial-port driver/setup payload.
- **Distribution form:** selected unmodified upstream signed binaries under `resources/com0com/`, bundled separately from the TauTerm executable on Windows.
- **Upstream:** https://sourceforge.net/projects/com0com/
- **License:** GNU General Public License version 2 or later (GPL-2.0-or-later).
- **Full license text shipped with the payload:** `resources/com0com/COPYING-GPL-2.0.txt`.
- **Exact binary and corresponding-source provenance:** `resources/com0com/SOURCE.md`.

TauTerm does not link com0com into TauTerm-owned code. The upstream project publishes the corresponding 3.0.0.0 source archive next to the signed binary package. TauTerm's release workflow also downloads that exact official source archive, validates it as a ZIP, and publishes it as `com0com-3.0.0.0-source.zip` beside every TauTerm GitHub Release artifact. Changing the Windows distribution channel or com0com version still requires a fresh GPL source-delivery review.

### TCNOpen TRDP 3.0.0.0

- **Purpose:** native TRDP PD/MD implementation used by TauTerm's TRDP sidecar.
- **Distribution form:** vendored source under `src-tauri/vendor/tcnopen/`, compiled into the TauTerm-owned native sidecar.
- **Upstream:** https://sourceforge.net/projects/tcnopen/files/TRDP/3.0.0.0/
- **License:** Mozilla Public License 2.0 (MPL-2.0).
- **Full license:** `src-tauri/vendor/tcnopen/LICENSE`.
- **Source/provenance and downstream patch list:** `src-tauri/vendor/tcnopen/SOURCE.json`.

The vendored snapshot is the upstream 3.0.0.0 source plus the narrow downstream patch series declared in `SOURCE.json`. TauTerm keeps the covered source in the TauTerm source repository. Binary recipients can obtain the corresponding MPL-covered source from https://github.com/hamburger-os/TauTerm using the release tag matching their installed version, under `src-tauri/vendor/tcnopen/`.

**TRDPSpy is not included.**

### riperf3 0.8.0 (TauTerm vendored fork)

- **Purpose:** iperf3 wire-compatible implementation used by the iperf session.
- **Distribution form:** modified vendored Rust source under `src-tauri/vendor/riperf3/`.
- **Upstream:** https://github.com/therealevanhenry/riperf3
- **License:** MIT OR Apache-2.0.
- **License files:** `src-tauri/vendor/riperf3/LICENSE-MIT.txt` and `src-tauri/vendor/riperf3/LICENSE-APACHE.txt`.
- **TauTerm modifications:** `src-tauri/vendor/riperf3/VENDOR-NOTES.md`.

The vendored copy is intentionally not described as pristine upstream source; its TauTerm-specific changes are recorded beside it. For binary distribution, TauTerm elects the **MIT** option of the upstream `MIT OR Apache-2.0` grant and preserves the upstream MIT notice below:

> Copyright (c) Individual contributors
>
> Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the "Software"), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:
>
> The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

### Lua 5.4 runtime

TauTerm enables `mlua` with the `lua54` and `vendored` features, so a Lua 5.4 runtime is built into the application through the Cargo dependency chain. The exact resolved build source is recorded by `src-tauri/Cargo.lock` (currently `lua-src 547.0.0`).

Lua 5.4 is distributed under the MIT license. Official license and reference manual:

- https://www.lua.org/license.html
- https://www.lua.org/manual/5.4/

Copyright © 1994–2026 Lua.org, PUC-Rio.

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the "Software"), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

### Reviewed package-managed MPL-2.0 crates

The current Cargo graph also contains unmodified MPL-2.0 crates that are not maintained as TauTerm forks:

- `cssparser 0.36.0`
- `cssparser-macros 0.6.1`
- `dtoa-short 0.3.5`
- `option-ext 0.2.0`
- `selectors 0.36.1`
- `serialport 4.10.0`

These versions and their MPL-2.0 expressions are explicitly reviewed by `scripts/check-cargo-licenses.js`. An upgrade or license-expression change fails closed and requires another review. Their resolved package metadata and available license/notice texts are included in the generated dependency notice shipped with TauTerm.

## Runtime/system components not redistributed by TauTerm

### Npcap

On Windows, TRDP live capture dynamically uses a **user-installed** Npcap-compatible `wpcap.dll`. TauTerm does not bundle Npcap.

### libpcap

On Linux/macOS, TRDP live capture uses the **system-provided** libpcap at runtime. TauTerm does not bundle libpcap. Offline pcap/pcapng analysis does not require either Npcap or system libpcap.

## Package-managed dependencies

TauTerm also ships compiled/bundled output from ordinary Rust and npm dependencies such as Tauri, React, xterm.js, russh, and related libraries. Their exact resolved versions are locked in:

- `src-tauri/Cargo.lock`
- `package-lock.json`

Do not manually copy every transitive package into this file. Before a production bundle is assembled, `scripts/generate-third-party-notices.js` generates `THIRD_PARTY_DEPENDENCY_LICENSES.txt` from the resolved Cargo/npm dependency graph and the license/notice files present in installed source packages. The generated file is bundled with TauTerm and intentionally uses a conservative dependency set, including build/development dependencies where applicable.

When a dependency is newly vendored, materially modified, separately redistributed, copyleft-licensed, or otherwise needs a distribution obligation beyond ordinary package attribution, add it to this curated inventory and to the automated third-party checks.

## Distribution contract

`THIRD_PARTY_LICENSES.md`, the generated dependency notice, TauTerm's MIT/Apache license texts, and the TCNOpen MPL-2.0 license are bundled as application resources so binary recipients have the relevant terms and attribution inventory. Component-local license/provenance files remain beside redistributed or vendored material.

The mechanically checkable part of this contract is enforced by `scripts/check-third-party.js`. License interpretation and source-delivery obligations still require human review when a distribution model changes.
