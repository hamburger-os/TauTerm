# Vendored TCNOpen TRDP 3.0.0.0

This directory contains a source snapshot of the **TCNOpen TRDP 3.0.0.0** stack used by TauTerm's TRDP Node helper.

- Upstream project: https://sourceforge.net/projects/tcnopen/
- Official release: https://sourceforge.net/projects/tcnopen/files/TRDP/3.0.0.0/3.0.0.0.zip/download
- Upstream license: **Mozilla Public License 2.0 (MPL-2.0)**
- Snapshot tag: `tags/3.0.0.0`
- Text-source mirror used to materialize the snapshot in this repository: `HtoTheB/TCNOpen-Mirror`, branch `tag/3.0.0.0`, commit `379221f881c2abe4862cac9c7fc9b3557a25ae19`.

The mirror is only a transport for the public SVN tag. It is **not** a build-time or runtime dependency. Maintainers can verify/refresh this directory from the official SourceForge ZIP with `python scripts/vendor_tcnopen.py --check` or `--update`.

TauTerm does not modify these upstream source files. Platform/build adaptation is kept outside this directory in `src-tauri/native/CMakeLists.txt` and TauTerm-owned bridge sources.

## What is built

TauTerm's native helper builds the upstream TRDP core PD/MD stack:

- `tlc_if.c`, `tlp_if.c`, `tlm_if.c`
- `trdp_pdcom.c`, `trdp_mdcom.c`, `trdp_utils.c`, `trdp_stats.c`
- VOS memory, socket, thread and shared-memory sources for POSIX or Windows

The vendored public API headers and `trdp-config.xsd` are retained for API/schema traceability. Optional TCNOpen TAU/TTI/DNR/SOA/TSN code is not linked into the first TRDP session implementation.

**TRDPSpy is not included.**
