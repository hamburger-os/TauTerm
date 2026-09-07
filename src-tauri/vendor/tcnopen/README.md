# Vendored TCNOpen TRDP 3.0.0.0

This directory contains the **TCNOpen TRDP 3.0.0.0** source used by TauTerm's TRDP Node/native sidecar.

- Upstream project: https://sourceforge.net/projects/tcnopen/
- Official release: https://sourceforge.net/projects/tcnopen/files/TRDP/3.0.0.0/3.0.0.0.zip/download
- Upstream license: **Mozilla Public License 2.0 (MPL-2.0)**
- Snapshot tag: `tags/3.0.0.0`
- Text-source mirror used to materialize the snapshot in this repository: `HtoTheB/TCNOpen-Mirror`, branch `tag/3.0.0.0`, commit `379221f881c2abe4862cac9c7fc9b3557a25ae19`.

The mirror is only a transport for the public SVN tag. It is **not** a build-time or runtime dependency. Maintainers can verify/refresh this directory from the official SourceForge ZIP with `python scripts/vendor_tcnopen.py --check` or `--update`.

The vendored tree is **not byte-for-byte pristine upstream source**. TauTerm keeps the upstream 3.0.0.0 source plus the ordered, narrow downstream patch series declared in [SOURCE.json](SOURCE.json). Those patches repair source-history text encoding and scope an MSVC warning around an upstream flexible-array declaration; TauTerm-owned platform/build adaptation remains outside the covered tree in `src-tauri/native/CMakeLists.txt` and TauTerm bridge sources.

## What is built

TauTerm's native helper builds the upstream TRDP core PD/MD stack:

- `tlc_if.c`, `tlp_if.c`, `tlm_if.c`
- `trdp_pdcom.c`, `trdp_mdcom.c`, `trdp_utils.c`, `trdp_stats.c`
- VOS memory, socket, thread and shared-memory sources for POSIX or Windows

The vendored public API headers and `trdp-config.xsd` are retained for API/schema traceability. Optional TCNOpen TAU/TTI/DNR/SOA/TSN code is not linked into the first TRDP session implementation.

The full MPL-2.0 text is retained in [LICENSE](LICENSE), and the source/provenance contract is recorded in [SOURCE.json](SOURCE.json).

**TRDPSpy is not included.**
