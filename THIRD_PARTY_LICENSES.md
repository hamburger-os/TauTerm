# Third-party licenses

TauTerm itself remains licensed under **MIT OR Apache-2.0**. The components below are separate third-party works and remain under their respective licenses.

## TCNOpen TRDP 3.0.0.0

TRDP Node support can be enabled by running `scripts/bootstrap-trdp.ps1` on Windows or `scripts/bootstrap-trdp.sh` on Linux/macOS. These scripts download the fixed **TCNOpen TRDP 3.0.0.0** source release from the official TCNOpen SourceForge project and build a separate helper executable, `tauterm-trdp-bridge`.

TCNOpen TRDP source is licensed under the **Mozilla Public License 2.0 (MPL-2.0)**. TauTerm does not relicense or remove notices from TCNOpen source. The downloaded source is kept outside the committed TauTerm source tree under `.cache/tcnopen-3.0.0.0/`, and the native helper links to the TCNOpen TRDP library as a separate component.

When redistributing a build containing `tauterm-trdp-bridge`, distributors must preserve the applicable TCNOpen notices and comply with the MPL-2.0 source-availability requirements for the MPL-covered TCNOpen files. TauTerm-owned bridge code remains MIT OR Apache-2.0.

TCNOpen project: https://sourceforge.net/projects/tcnopen/
MPL-2.0: https://www.mozilla.org/MPL/2.0/

**TRDPSpy is not used or distributed by TauTerm.** TRDPSpy has different licensing and is deliberately outside this integration.

## Npcap / libpcap

TauTerm does **not** redistribute Npcap. On Windows, live TRDP Monitor capture dynamically loads an Npcap installation already present on the user's machine (`wpcap.dll`). Offline `.pcap` / `.pcapng` analysis does not require Npcap.

On Linux/macOS, the native helper dynamically loads the system libpcap when live capture is requested.
