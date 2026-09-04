# TRDP Sessions

> Current `master` ships first-party TRDP Node and Monitor sessions backed by vendored **TCNOpen TRDP 3.0.0.0**. This document describes the shipped behavior, build/runtime dependencies, and intentionally unsupported boundaries.

## Session model

TauTerm exposes two top-level TRDP session modes.

### Node

A Node owns one TCNOpen application session per enabled TauTerm link and can host multiple TRDP roles at the same time:

- **PD Publisher**
- **PD Subscriber**
- **PD Request**
- **MD Notify**
- **MD Request**
- **MD Listener / Replier**

The session view is split into Overview, Publishers, Subscribers, Messages and Traffic. Objects are created in **Stopped** state. Transmitting objects are never auto-started when a saved TauTerm configuration or Workspace is restored; use **Start** or **Send** explicitly. PD Request is a one-shot **Send** action in the UI and remains Stopped; the native side may keep a temporary subscriber handle for the reply window, which is replaced on the next Send and cleaned when the object/session is removed.

### Monitor

Monitor is passive. It can:

- capture one or two interfaces live;
- inspect PD and MD traffic;
- preserve which TauTerm capture link (A/B) observed a frame;
- open `.pcap` and `.pcapng` files;
- save inspected raw frames as `.pcapng`;
- reassemble MD/TCP streams during live capture.

A passive monitor only sees traffic presented to the selected NIC. On a switched network, use a SPAN/mirror port, TAP, or other capture arrangement when the traffic is not already delivered to the host.

## Link A/B and redundancy

TauTerm's **Link A / Link B** selection is a physical/interface choice. It is deliberately separate from TRDP redundancy metadata such as `redId` and Leader/Follower state.

Do not interpret:

- Link A = Leader
- Link B = Follower

They are independent concepts. A PD object can use A, B or Both, and Monitor can capture one or two interfaces. TauTerm keeps link provenance instead of implicitly de-duplicating A/B packets or selecting a master copy.

## Ports

The standard defaults are:

| Traffic | Default |
|---|---:|
| PD UDP | 17224 |
| MD UDP | 17225 |
| MD TCP | 17225 |

Node and offline Monitor allow advanced custom port values. Live Monitor offers **Auto** and **Custom** capture-filter modes. Auto starts with the standard ports (PD UDP/17224, MD UDP/TCP/17225) and regenerates the BPF expression from the currently configured PD/MD ports; Custom passes the user-supplied libpcap/Npcap BPF expression unchanged.

## PD behavior

TauTerm exposes ComID, source/destination addressing, cycle/timeout values, topology counters, redundancy settings, raw payload and decoded Dataset values when an XML schema is available.

Diagnostics include the observed sequence, packet count, missed sequence estimates, interval min/average/max, jitter relative to a known publisher/XML cycle, payload size and result errors. Live/offline Monitor also validates the TRDP header CRC and the compatible protocol-version byte. Topology counters are shown verbatim in Monitor; topology validity is only judged by an active Node where TCNOpen has the application-session topology context.

Subscriber/PD Request timeout mode is explicit: **Auto** sends `timeout_us=0` and lets TCNOpen apply the application-session default; **Custom** sends the configured timeout; **Disabled** sends TCNOpen's `TRDP_INFINITE_TIMEOUT`. Timeout behavior independently chooses Keep last value or Set to zero. For official XML, `<pd-parameter timeout="0">` or an omitted timeout means Disabled, while a positive timeout becomes Custom; `validity-behavior` is preserved (XSD default: zero). TauTerm does not derive a universal `3 × cycle` timeout.

PD Request supports an independent Reply ComID and Reply IP. A zero Reply ComID means "use the request ComID"; `0.0.0.0` Reply IP resolves to the selected link's local address.

## MD behavior

Messages expose message type, ComID, source/destination, MD Session UUID, request/reply latency, reply/user status, reply counts, timeout information, URI fields and UDP/TCP transport. An active Node can also show TCNOpen-local expected-reply/session counters. Passive Monitor derives only fields present on the wire plus observed replies for the same UUID; it does not invent expected-reply state that is absent from the MD header.

Supported active workflows include:

- Notify
- Request / Reply
- Listener / Replier
- ReplyQuery / Confirm
- Abort of an active MD session

Source and destination URI fields are available for MD objects.

## XML, Workspace and Dataset payloads

TauTerm distinguishes two file formats.

### TRDP XML

Import a TCNOpen-style TRDP XML file to obtain:

- ComID → Dataset mappings;
- telegram cycle/timeout metadata;
- PD/MD port configuration;
- source/destination templates;
- Dataset element definitions;
- SDT presence metadata.

The importer is intentionally **read/decode oriented**. TauTerm does not rewrite the full input XML as a general-purpose TCNOpen configuration editor. Import Preview classifies telegrams from explicit `<pd-parameter>` / `<md-parameter>` elements. Telegrams with neither are marked Unknown; telegrams with both are marked Ambiguous. The automatic template action only creates stopped **PD Subscriber** templates; MD telegrams remain visible in Preview because XML alone does not tell TauTerm whether the local node should act as requester, notifier or listener/replier.

### TauTerm Workspace JSON

TauTerm's own Workspace format is:

`tauterm-trdp-workspace/v1`

Workspace import restores TauTerm object configuration and keeps every object Stopped. It is not an alternative serialization of the official TRDP XML schema.

### Raw HEX is the wire truth source

Each active object ultimately sends a raw byte payload. When a Dataset schema is known, the Structured Dataset editor can convert in both directions:

- **HEX → Fields**
- **Fields → HEX**

The encoder/decoder handles network byte order, fixed/dynamic arrays, nested Datasets (including the XSD minimum Dataset ID 1000), supported primitive types, and XML scale/offset metadata. The generated HEX is written back to the object and remains the final wire representation.

## Capture files

Offline capture understands classic pcap and pcapng for supported link types and IPv4 UDP/TCP TRDP traffic. The offline Rust parser decodes captured frames/segments; TCP stream reassembly is currently provided by the live Monitor path, not by offline pcap replay.

TauTerm saves pcapng by default. When live traffic has A/B provenance, the writer creates separate pcapng Interface Description Blocks keyed by TauTerm link and datalink and stores the link label as the interface name. Reopening the saved pcapng therefore preserves A/B provenance.

Supported capture link types include Ethernet, Linux cooked capture (SLL/SLL2), DLT_NULL and raw IPv4.

## Live capture requirements

### Windows

Live Monitor dynamically loads **Npcap** through `wpcap.dll`.

TauTerm does **not** bundle or install Npcap. Install Npcap separately if live capture is required. Offline pcap/pcapng analysis does not require Npcap.

### Linux / macOS

Live Monitor dynamically loads the system **libpcap**. Install a platform libpcap package when it is not already available.

Offline pcap/pcapng analysis does not require libpcap.

Capture permissions are controlled by the operating system. Prefer normal platform capture-permission configuration instead of running the whole TauTerm application as root/Administrator.

## SDT boundary

TauTerm v1 can detect and preserve SDT-related metadata/raw payloads found in imported TRDP material, but it does **not** validate SDTv2/SDTv4 safety semantics.

TauTerm is not a safety-certified validator and must not be used as evidence that an SDT telegram or safety application is safe/certified.

## TCNOpen source and licensing

The protocol engine is built from a repository-local source snapshot under:

`src-tauri/vendor/tcnopen/`

The snapshot is TCNOpen TRDP **3.0.0.0** and remains covered by **MPL-2.0**. Upstream notices are retained. TauTerm-owned Rust, TypeScript, bridge C code and CMake files remain **MIT OR Apache-2.0**.

Normal builds do not download TCNOpen and do not require a separately installed TCNOpen SDK.

Source provenance is recorded in:

- `src-tauri/vendor/tcnopen/README.md`
- `src-tauri/vendor/tcnopen/SOURCE.json`
- [THIRD_PARTY_LICENSES.md](../THIRD_PARTY_LICENSES.md)

**TRDPSpy is not used, linked, vendored or distributed.**

## Building the native helper

TRDP source builds require:

- CMake 3.20+
- a C compiler for the target platform
- the normal TauTerm Rust/Node/Tauri prerequisites

Build both the native bridge and reference peer with:

### Linux / macOS

```bash
bash scripts/bootstrap-trdp.sh
```

Outputs:

- `src-tauri/binaries/tauterm-trdp-bridge`
- `tools/trdp-test-peer/bin/trdp-test-peer`

### Windows x64

```powershell
./scripts/bootstrap-trdp.ps1
```

Outputs:

- `src-tauri/binaries/tauterm-trdp-bridge.exe`
- `tools/trdp-test-peer/bin/trdp-test-peer.exe`

When building a Tauri installer/package, `beforeBundleCommand` runs the same bootstrap automatically and stages the helper using Tauri `bundle.externalBin` with the target-triple sidecar name. Developers who want to exercise TRDP from `tauri dev` should run the bootstrap once first.

See [BUILDING.md](BUILDING.md) for the complete application build environment.

## Samples

The repository includes:

- `samples/trdp/pd-unicast.xml`
- `samples/trdp/pd-multicast.xml`
- `samples/trdp/pd-dataset.xml`
- `samples/trdp/md-request-reply.xml`
- `samples/trdp/full-node.xml`
- `samples/trdp/tauterm-lab-profile.json`

These are lab examples, not production railway configuration templates.

## Reference peer and interoperability CI

`tools/trdp-test-peer/` contains a small TCNOpen C peer used for interoperability testing. The bootstrap script builds it from the same vendored 3.0.0.0 source tree.

The `TRDP Native` GitHub Actions workflow validates:

- native TCNOpen + bridge builds on Windows x64, Linux x86_64 and macOS Apple Silicon;
- Linux PD Publish → Subscribe interoperability;
- Linux MD UDP Request → Reply;
- Linux MD TCP Request → Reply;
- Linux ReplyQuery → Confirm;
- Linux Tauri `.deb` packaging contains the TRDP sidecar.

The reference peer is a test utility, not a simulator or second protocol implementation.

## Current limitations

The first shipped implementation intentionally does not claim:

- SDTv2/SDTv4 safety validation or certification;
- automatic A/B de-duplication or master-selection logic;
- Windows ARM64, Linux ARM64 or macOS Intel release-grade support;
- bundled Npcap;
- a full round-trip TRDP XML authoring/export system.

These are explicit boundaries, not implied capabilities.
