# TauTerm Product Strategy

> This document records TauTerm's long-term product direction. It is broader than the implementation roadmap and should change only when product decisions change.

## 1. Product vision

**TauTerm is an open, local-first engineering workbench for connected systems.**

It should let an engineer connect to, observe, understand, automate and reproduce a system that spans remote computers, embedded devices, network protocols and physical engineering instruments while keeping that engineering context together.

The public product line remains:

> **TauTerm — one terminal for the server room and the lab bench.**

The broader product category is:

> **The open engineering workbench for connected systems.**

## 2. Primary users

TauTerm has three related user groups with different roles in the product strategy.

1. **Embedded developers are the product root.** Device bring-up, serial communication, binary data, real-time signals and hardware-adjacent workflows remain first-class.
2. **Connected-system and device R&D engineers are the product center.** They work across devices, Linux services, network protocols, logs, test tools and automation in the same engineering task.
3. **Industrial and railway engineering teams are the primary long-term commercial customers.** They need offline operation, long-running stability, traceability, specialist protocols, repeatable workflows, controlled deployment and support.

TauTerm can also serve network and infrastructure engineers where those workflows naturally intersect with connected-system engineering.

## 3. Product principles

### 3.1 Local-first by design

Core engineering workflows must work without an account, cloud service or Internet connection.

SSH, Serial, network debugging, Recording/Replay, Signal Lab, Data Lens, protocol analysis and automation should remain usable in laboratories, factories, railway environments and isolated networks.

Optional online services may later provide synchronization, licensing, collaboration or distribution, but they must not be runtime dependencies for the engineering workbench.

### 3.2 Complete open core, paid professional value

The open-source Community/Core edition should remain a complete and genuinely useful engineering tool. Basic SSH/SFTP, Serial, TCP/UDP, local shell, protocol debugging, scripting and extensibility should not be artificially restricted to force an upgrade.

Commercial products should focus on high-value professional workflows, official advanced modules, team collaboration, enterprise governance, support and industry-specific capabilities.

Commercial modules may use separate proprietary licensing while the Community/Core repository remains MIT OR Apache-2.0.

### 3.3 Engineering context before protocol count

New capabilities should normally strengthen at least one of these areas:

- preserve engineering context;
- correlate information across sessions or instruments;
- turn raw bytes into useful engineering meaning;
- improve repeatability or automation;
- provide meaningful industrial depth;
- integrate a physical engineering instrument into the same workflow.

Protocols that do not materially strengthen these goals should prefer the plugin/extension path instead of expanding the core indefinitely.

### 3.4 Native TauTerm workflows

TauTerm should prioritize its own Workspace, data model and engineering workflow instead of building migration-oriented importers for unrelated application formats.

Interoperability with open ecosystem standards can still be considered when it provides durable engineering value.

### 3.5 Industrial depth without narrowing the product

Railway and industrial engineering are strategic verticals, but the horizontal product remains the TauTerm engineering workbench.

Deep first-party capabilities such as TRDP should demonstrate professional depth while remaining part of a broader connected-system workflow.

### 3.6 Software and instruments form one platform

TauTerm is intended to become the common desktop software for first-party engineering instruments, beginning with a possible future CAN analyzer and potentially extending to additional analyzers.

Each instrument should plug into the same data and workflow model rather than require an isolated desktop application.

First-party hardware should receive the most integrated experience, while the architecture can still support third-party or generic adapters where that improves the ecosystem.

## 4. Shared engineering data model

Long-term product coherence should come from a shared event pipeline rather than separate feature silos.

```text
Transport / Instrument
        ↓
     Raw Event
        ↓
     Framing
        ↓
     Decoder
        ↓
 Structured Event / Signal
   ├─ Terminal / Packet View
   ├─ Signal Lab
   ├─ Data Lens
   ├─ Unified Timeline
   ├─ Recorder / Replay
   └─ Automation
```

The model should eventually work across SSH output, journald, Serial, TCP/UDP, TRDP, CAN and first-party instruments.

Raw information should remain available close enough to the source that recordings can be re-decoded or re-analyzed later.

## 5. Strategic capability pillars

### 5.1 Foundation

**Goal:** make TauTerm comfortable enough to remain open all day as a primary engineering tool.

Priorities include:

- Local Shell;
- split panes;
- SSH tunnels and jump hosts;
- Workspace foundations rather than simple session groups;
- long-running stability and explicit performance budgets;
- excellent session/configuration ergonomics;
- cross-platform release quality.

These capabilities establish daily-driver quality and support everything that follows.

### 5.2 Engineering Memory

**Goal:** make debugging reproducible rather than disposable.

TauTerm should develop structured Recording/Replay that preserves engineering evidence near the raw event stream.

A recording should retain, where applicable:

- timestamp and clock domain;
- session/instrument identity;
- transport and peer;
- TX/RX direction;
- raw bytes or samples;
- decoded/structured fields;
- markers and annotations;
- automation actions;
- transfer and test events.

Replay should allow engineers to inspect a problem again without reconnecting to the original equipment and, where possible, re-run decoding with newer decoder logic.

The larger goal is a **Unified Timeline** that correlates events from multiple sessions and instruments around the same engineering event.

### 5.3 Signal Lab

**Goal:** provide a complete real-time numerical-data workflow inside TauTerm.

Signal Lab should become strong enough that many embedded workflows no longer require a separate real-time plotting application.

Target capabilities include:

- high-throughput real-time plotting;
- bounded resource use during long capture sessions;
- multi-channel curves;
- zoom, cursors and measurements;
- FFT and statistics;
- pause, inspect and resume;
- export;
- FireWater / JustFloat compatibility;
- custom framing and numerical extraction;
- integration with recorded and replayed data.

Signal Lab answers:

> **What is the signal doing over time?**

### 5.4 Data Lens

**Goal:** turn raw protocol and device data into reusable engineering meaning.

A binary packet may become:

```text
traction_status
├─ vehicle_id: 3
├─ speed: 61.2 km/h
├─ brake_pressure: 2.8 bar
├─ traction_current: 412 A
└─ crc: OK
```

Decoded fields should eventually be reusable for:

- filtering and search;
- tables and packet inspection;
- plots and gauges;
- statistics;
- automation triggers;
- Timeline correlation;
- export and reports.

The decoder model should be reusable across Serial, TCP/UDP, TRDP, CAN and future instruments.

Data Lens answers:

> **What do these bytes mean?**

Signal Lab and Data Lens share the same data pipeline but serve different engineering questions.

### 5.5 Industrial Depth

**Goal:** solve complete professional workflows in selected industrial domains.

TRDP is a first-party strategic protocol because it serves a real railway engineering need.

Its long-term workflow can include:

- PD/MD inspection;
- COMID visibility and filtering;
- multicast/source analysis;
- sequence and timeout diagnostics;
- cycle/jitter/loss statistics;
- dataset decoding;
- recording and replay;
- correlation with Serial, SSH and journald events.

Other industrial protocols should be evaluated by workflow depth, engineering value and fit with the shared data model.

### 5.6 Instrument Platform

**Goal:** make TauTerm the common upper-computer environment for first-party analyzers.

A future CAN analyzer is the first likely candidate. It should appear as a first-class instrument/session using the same Workspace, Recorder, Unified Timeline, Data Lens, Signal Lab and Automation systems.

Every new instrument should gain the existing software platform, while every software capability should become useful across more instruments.

See [HARDWARE_ECOSYSTEM.md](HARDWARE_ECOSYSTEM.md).

### 5.7 Automation

**Goal:** move from isolated scripts to repeatable engineering workflows.

Lua remains an important expert runtime. A higher-level flow layer can later expose trigger / condition / action automation without requiring every user to write a full script.

Example:

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

Agent features must respect local-first operation and must not require industrial data to leave the user's environment.

### 5.8 Team and Enterprise

**Goal:** provide organizational value without changing the local-first engineering model.

Potential capabilities include:

- shared Workspaces;
- shared decoder and automation libraries;
- recording review and annotations;
- private plugin/instrument registries;
- secrets and policy controls;
- audit logs;
- offline/floating/site licensing;
- controlled updates and LTS releases;
- enterprise support.

See [COMMERCIALIZATION.md](COMMERCIALIZATION.md).

## 6. Roadmap model

The roadmap is capability-led. Version numbers are delivery vehicles rather than the definition of the strategy.

| Stage | Intended outcome |
|---|---|
| **Foundation** | Daily-driver quality: Local Shell, split panes, SSH tunnel/jump-host support, Workspace foundation, release/performance quality |
| **Engineering Memory** | Structured recording, replay, markers, search and Unified Timeline |
| **Signal Lab** | High-performance plotting, FFT/statistics, FireWater/JustFloat and real-time numerical workflows |
| **Data Intelligence** | Framing/decoder SDK, Data Lens, reusable fields, filters, visualizations and triggers |
| **Industrial & Instruments** | Deep TRDP workflows, offline/long-run industrial quality and first-party instrument integration such as a future CAN analyzer |
| **Automation & Teams** | Flow automation, Lua/CLI/MCP evolution, collaboration, governance and enterprise deployment |

Roadmap stages describe direction, not release commitments.

## 7. Product decision filter

A proposed feature should answer these questions before becoming a core priority:

1. What engineering problem does it solve?
2. Does it strengthen the shared Workspace or data model?
3. Does it improve observation, understanding, reproducibility or automation?
4. Is it broadly reusable, or should it be a plugin/module?
5. Does it preserve local-first operation?
6. If it introduces a new protocol or instrument, can it participate in Recording, Timeline, Data Lens, Signal Lab or Automation where appropriate?

## 8. Explicit non-goals

Unless product evidence changes the direction, TauTerm should not prioritize:

- migration-oriented importers for unrelated application formats;
- protocol accumulation for its own sake;
- mandatory cloud accounts for engineering features;
- generic expansion into unrelated desktop-tool categories;
- isolated instrument UIs that cannot participate in the shared data, recording and automation model.

## 9. Compounding product value

TauTerm's long-term product advantage should come from the way its capabilities reinforce one another:

**local-first engineering + remote systems + embedded devices + industrial protocols + first-party instruments + structured Recording/Replay + Signal Lab + Data Lens + Automation.**

The goal is a coherent engineering environment in which new protocols, new instruments and new analysis capabilities all strengthen the same Workspace and data model.