# TauTerm Product Strategy

> This document records the product direction behind TauTerm. It is intentionally broader than the implementation roadmap and should be updated when product decisions change.

## Vision

**TauTerm is the open, local-first engineering workbench for connected systems.**

It should let an engineer connect to, observe, understand, automate and reproduce a system that spans remote computers, embedded devices, network protocols and physical test instruments without having to split that context across unrelated tools.

The public product line remains:

> **TauTerm — one terminal for the server room and the lab bench.**

The category is broader than a terminal emulator:

> **The open engineering workbench for connected systems.**

TauTerm should earn adoption because its combined workflow is better, not because it imports configuration from competing applications.

## Who TauTerm is for

TauTerm has three closely related audiences rather than one generic developer persona.

1. **Embedded developers are the product root.** Device bring-up, serial communication, binary data, real-time signals and hardware-adjacent workflows must remain first-class.
2. **Connected-system / device R&D engineers are the product center.** These users work across devices, Linux services, network protocols, logs, test tools and automation at the same time. TauTerm should remove the boundaries between those contexts.
3. **Industrial and railway engineering teams are the primary long-term commercial customers.** They need offline operation, long-running stability, traceability, specialist protocols, repeatable test workflows, controlled deployment and support.

TauTerm may also be useful to DevOps and network engineers, but it should not become a generic IT administration suite at the expense of embedded and industrial engineering depth.

## Product principles

### 1. Local-first by design

Core engineering workflows must work without an account, cloud service or Internet connection.

SSH, serial, network debugging, recording/replay, Signal Lab, Data Lens, protocol analysis and automation should remain usable in isolated laboratories, railway networks, factories and air-gapped environments.

Cloud services may later add optional synchronization, licensing, collaboration or distribution, but they must not be a runtime dependency for the engineering workbench.

### 2. Complete open core, paid professional value

The open-source Community/Core edition should remain a complete and genuinely useful engineering tool. Basic SSH/SFTP, Serial, TCP/UDP, local shell, protocol debugging, scripting and extensibility should not be artificially crippled to force upgrades.

Commercial products should charge for high-value professional workflows, official advanced modules, team collaboration, enterprise governance, support and industry-specific capabilities.

Commercial modules may be proprietary even while the Community/Core repository remains MIT OR Apache-2.0.

### 3. Engineering context beats protocol count

TauTerm does not win by accumulating the longest protocol checklist.

A new feature should normally improve at least one of these capabilities:

- preserve engineering context;
- correlate information across sessions or instruments;
- turn raw bytes into useful engineering meaning;
- improve repeatability or automation;
- provide meaningful industrial depth;
- integrate a physical engineering instrument into the same workflow.

Protocols that do not strengthen those goals should prefer the plugin/extension path instead of expanding the core indefinitely.

### 4. Independent product, not a migration assistant

TauTerm will not prioritize importing configuration from competing terminal, serial or network-debugging products.

Users should choose TauTerm because its own Workspace and engineering workflows are worth setting up. Supporting an ecosystem standard such as OpenSSH configuration can be considered separately when it improves interoperability rather than competitor migration.

### 5. Industrial depth without narrowing the brand

Railway and industrial engineering are strategic verticals, but TauTerm should not become a railway-only product.

The horizontal product remains the TauTerm engineering workbench. Deep first-party vertical capabilities such as TRDP demonstrate professional depth and can later be complemented by railway, industrial or embedded solution packs.

### 6. Software and instruments form one platform

TauTerm is intended to become the common desktop software for first-party engineering instruments, beginning with a possible future CAN analyzer and potentially extending to additional analyzers.

Each instrument should plug into the same data and workflow model rather than ship with an isolated one-off desktop application.

First-party hardware should receive the best zero-configuration experience, but TauTerm should remain architecturally capable of supporting third-party or generic adapters where that improves the ecosystem.

## The core data model

Long-term differentiation should come from a common event pipeline rather than separate feature silos.

```text
Transport / Instrument
        ↓
     Raw Event
        ↓
     Framing
        ↓
     Decoder
        ↓
 Structured Event
   ├─ Terminal / Packet View
   ├─ Signal Lab
   ├─ Data Lens
   ├─ Unified Timeline
   ├─ Recorder / Replay
   └─ Automation
```

This model should eventually work across SSH output, journald, Serial, TCP/UDP, TRDP, CAN and first-party instruments.

## Strategic product pillars

### Foundation — become a daily driver

Before advanced differentiation matters, TauTerm must be comfortable enough to remain open all day.

Priorities include:

- Local Shell;
- split panes;
- SSH tunnels and jump hosts;
- Workspace foundations rather than simple session groups;
- long-running stability and performance budgets;
- excellent session/configuration ergonomics;
- cross-platform release quality.

These are table stakes, not the long-term moat.

### Engineering Memory — make debugging reproducible

A terminal log is not enough. TauTerm should develop structured recording and replay that preserves engineering evidence close to the raw event stream.

A recording should be able to retain, where applicable:

- timestamp and clock domain;
- session/instrument identity;
- transport and peer;
- TX/RX direction;
- raw bytes;
- decoded/structured fields;
- markers and annotations;
- automation actions;
- transfer or test events.

Replay should allow engineers to analyze a problem again without reconnecting to the real equipment, including re-decoding data with an updated decoder where possible.

The larger goal is a **Unified Timeline** that correlates events from multiple sessions and instruments around the moment a fault occurred.

### Signal Lab — replace dedicated serial plotting workflows

Signal Lab is the real-time numerical-data path and should become strong enough that an embedded engineer who currently needs TauTerm plus a dedicated VOFA+-style plotting tool can use TauTerm alone.

Target capabilities include:

- high-throughput real-time plotting;
- long capture sessions with bounded resource use;
- multi-channel curves;
- zoom, cursors and measurements;
- FFT and statistics;
- pause, inspect and resume;
- export;
- FireWater / JustFloat compatibility;
- custom framing and numerical extraction;
- later integration with recorded/replayed data.

Signal Lab answers: **what is the signal doing over time?**

### Data Lens — turn bytes into engineering meaning

Data Lens is different from Signal Lab. It should decode structured protocol or device data into named engineering fields and make those fields reusable across the product.

For example, a binary packet may become:

```text
traction_status
├─ vehicle_id: 3
├─ speed: 61.2 km/h
├─ brake_pressure: 2.8 bar
├─ traction_current: 412 A
└─ crc: OK
```

Decoded fields should eventually be usable for:

- filtering and search;
- tables and packet inspection;
- plots and gauges;
- statistics;
- automation triggers;
- Timeline correlation;
- export and reports.

The decoder model should be reusable across Serial, TCP/UDP, TRDP, CAN and future instruments.

Signal Lab answers **what is the signal doing?** Data Lens answers **what do these bytes mean?**

### Industrial Depth — solve real professional workflows

TRDP is a first-party strategic protocol because it serves a real railway engineering need, not because TauTerm is collecting protocol badges.

TRDP should ultimately grow beyond raw send/receive toward professional analysis such as:

- PD/MD inspection;
- COMID visibility and filtering;
- multicast/source analysis;
- sequence and timeout diagnostics;
- cycle/jitter/loss statistics;
- dataset decoding;
- recording and replay;
- correlation with Serial, SSH and journald events.

Other industrial protocols should be evaluated by the same standard: depth and workflow value matter more than quantity.

### Instrument Platform — one upper computer for many analyzers

TauTerm should become the shared desktop environment for future first-party analyzers instead of creating a new application for every hardware product.

A future CAN analyzer is the first obvious candidate. It should appear as another first-class instrument/session that can use the same Workspace, Recorder, Timeline, Data Lens, Signal Lab and Automation systems.

This creates compounding value: every new instrument gains the existing software platform, and every software capability becomes more useful as additional instruments join the platform.

See [HARDWARE_ECOSYSTEM.md](HARDWARE_ECOSYSTEM.md).

### Automation — move from scripts to engineering workflows

Lua remains an important expert runtime, but automation should become accessible without requiring every user to write a full script.

A future flow layer can express ideas such as:

```text
WHEN Serial matches "READY"
THEN SSH run "./start-backend.sh"

WHEN TRDP field timeout == true
THEN mark recording
AND query journald
```

Potential layers include:

- Lua API v2;
- trigger / condition / action flows;
- CLI automation;
- MCP/agent access with explicit permissions and auditability.

Agent features must respect local-first operation and should never require sending industrial data to a cloud model.

### Team / Enterprise — monetize organizational value

Long-term paid organization features can include:

- shared Workspaces;
- shared decoder and automation libraries;
- recording review and annotations;
- private plugin/instrument registries;
- encrypted secrets and policy controls;
- audit logs;
- offline/floating/site licensing;
- controlled updates and LTS releases;
- enterprise support.

## Roadmap structure

The roadmap should be capability-led rather than a list of protocols tied prematurely to version numbers.

| Stage | Outcome |
|---|---|
| **Foundation** | Daily-driver quality: Local Shell, split panes, SSH tunnel/jump-host support, Workspace foundation, release/performance quality |
| **Engineering Memory** | Structured recording, replay, markers, search and Unified Timeline |
| **Signal Lab** | High-performance plotting, FFT/statistics, FireWater/JustFloat and real-time numerical workflows |
| **Data Intelligence** | Framing/decoder SDK, Data Lens, reusable fields, filters, visualizations and triggers |
| **Industrial & Instruments** | Deep TRDP workflows, offline/long-run industrial quality and first-party instrument integration such as a future CAN analyzer |
| **Automation & Teams** | Flow automation, Lua/CLI/MCP evolution, collaboration, governance and enterprise deployment |

Version numbers remain delivery vehicles, not the definition of the strategy.

## Explicit non-goals

Unless evidence changes the strategy, TauTerm should not prioritize:

- competitor configuration importers;
- protocol accumulation for its own sake;
- mandatory cloud accounts for engineering features;
- generic RDP/VNC/database-client expansion simply to become an all-in-one desktop toolbox;
- isolated instrument UIs that cannot participate in the shared data, recording and automation model.

## The moat

The desired competitive advantage is not any individual checkbox. It is the combination of:

**local-first engineering + servers + embedded devices + industrial protocols + first-party instruments + structured recording/replay + Signal Lab + Data Lens + automation.**

A competitor can copy a protocol or a chart. It is substantially harder to copy a coherent workflow and data model that spans the server room, the lab bench and physical engineering instruments.