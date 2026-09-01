# TauTerm Commercialization Strategy

> This document records business hypotheses for TauTerm. It is not a published price list or licensing commitment. Packaging and pricing should be validated with real users and customers before becoming product policy.

## 1. Commercial objective

TauTerm should become financially sustainable while keeping the open-source product complete, trustworthy and useful.

Commercial value should come from areas where TauTerm provides clear professional or organizational leverage:

- advanced engineering analysis and reproducibility;
- official first-party instruments and instrument integrations;
- specialist industrial capabilities;
- team workflows and governance;
- controlled offline deployment and licensing;
- support, maintenance and long-term releases.

Basic engineering connectivity should remain a strong part of the Community edition.

## 2. Business model

The intended model is:

> **Open engineering core + commercial professional value + first-party hardware + support/services.**

The current Community/Core repository can remain MIT OR Apache-2.0. Optional commercial modules can use separate proprietary licensing behind clean package, repository or plugin boundaries.

The business should compound value through:

- product quality and release execution;
- the TauTerm brand;
- maintained official modules;
- first-party hardware integration;
- industrial domain depth;
- long-lived data/workflow compatibility;
- deployment and support capabilities.

## 3. Packaging model

### 3.1 TauTerm Community

**Purpose:** adoption, trust, ecosystem growth and a complete daily engineering baseline.

Expected baseline capabilities include:

- SSH/SFTP;
- Serial;
- TCP/UDP network debugging;
- TFTP/Telnet/iPerf;
- Local Shell when available;
- practical Workspace/session management;
- Lua scripting and extension/plugin foundations;
- useful baseline recording and data inspection;
- practical baseline connectivity for purchased first-party instruments.

Community should be good enough for an engineer to use TauTerm as a daily tool without purchasing an upgrade.

### 3.2 TauTerm Professional

**Purpose:** individual engineers who benefit from deeper analysis, reproducibility and automation.

Potential paid capabilities:

- advanced structured Recording/Replay;
- synchronized multi-session Unified Timeline;
- advanced Signal Lab analysis;
- advanced Data Lens and decoder-authoring workflows;
- richer report/export workflows;
- large and long-duration recording management;
- advanced Tau Flow automation;
- selected official professional protocol/industry modules;
- optional local/BYOK AI-assisted workflows;
- individual support or release-channel benefits.

Initial pricing hypotheses to validate:

- annual individual license: approximately USD 79–129;
- perpetual individual license: approximately USD 249–399 with a defined update period;
- regional pricing where appropriate.

A perpetual option is important because engineering users may need stable offline use without an active runtime subscription.

### 3.3 TauTerm Industrial / Team

**Purpose:** railway, industrial, device-development and test organizations.

This tier should solve organizational deployment, governance and support requirements.

Potential capabilities:

- shared Workspaces;
- shared decoder and automation libraries;
- recording review, annotations and evidence packages;
- private plugin/instrument registry;
- role and policy controls;
- centrally managed secrets where appropriate;
- audit trails;
- controlled update channels;
- offline activation;
- floating/concurrent licensing;
- lab/site licensing;
- LTS releases and defined maintenance windows;
- deployment assistance and enterprise support.

A preferred licensing structure for industrial deployments is:

- perpetual use of the purchased product version;
- an included update/support period;
- optional annual maintenance renewal for newer versions and continued support;
- no Internet requirement during normal engineering operation.

### 3.4 Test Bench / Station

Where TauTerm runs on a fixed validation rack, HIL bench, production tester or engineering station, a named-user license may not match the actual usage model.

Potential license units include:

- named engineer;
- floating/concurrent engineer;
- test bench/station;
- laboratory/site;
- enterprise agreement.

A station license should support controlled offline environments.

## 4. Licensing principles

Commercial licensing should preserve the local-first product model.

Preferred rules:

- no mandatory login for Community engineering workflows;
- offline activation path for Industrial customers;
- grace periods that do not interrupt active field or test work;
- purchased perpetual versions continue to run after maintenance expires;
- license state never blocks access to customer-owned recordings or data;
- enterprise deployments can pin a supported release;
- telemetry is opt-in and not required for licensing;
- purchased first-party hardware retains a useful baseline workflow without an active software subscription.

## 5. First-party hardware business

First-party analyzers can become a second major revenue stream while strengthening the software platform.

A future CAN analyzer is a natural first candidate. Additional instruments can later join the same TauTerm environment.

The intended model is:

1. hardware has sustainable standalone economics;
2. TauTerm Community provides a useful out-of-box hardware workflow;
3. Professional/Industrial software adds deeper recording, decoding, correlation, automation and reporting;
4. multiple Tau instruments share one Workspace and increase the value of the same software platform.

Potential bundles to validate later:

- instrument + Community software;
- instrument + Professional entitlement/update period;
- industrial instrument kit + Industrial maintenance/support;
- multi-instrument laboratory bundle;
- test-bench bundle with an offline station license.

Hardware pricing should be set only after BOM, certification, manufacturing, support, channel, warranty and inventory costs are understood.

See [HARDWARE_ECOSYSTEM.md](HARDWARE_ECOSYSTEM.md) for the instrument-platform design direction.

## 6. Official industrial modules

Commercial expansion should favor deep, maintained workflows rather than protocol quantity.

### 6.1 Railway module family

Potential value:

- advanced TRDP analysis;
- dataset/configuration tooling;
- cycle, jitter and loss diagnostics;
- railway-oriented evidence/report workflows;
- validated decoder libraries;
- long-term supported releases.

### 6.2 Industrial modules

Additional fieldbus or device protocols should be considered when they integrate naturally with Data Lens, Recorder, Unified Timeline and Automation and when there is a clear professional workflow to maintain.

### 6.3 Instrument modules

Advanced first-party instrument capabilities, calibrated analysis modules or specialist decoder packages may use commercial packaging while baseline instrument connectivity remains practical and durable.

## 7. Services and support

Services should accelerate adoption and reduce deployment risk without turning TauTerm into a customer-specific fork business.

Potential paid services:

- enterprise deployment assistance;
- custom protocol/decoder development;
- custom instrument integration;
- railway/industrial workflow integration;
- training;
- priority support;
- LTS and security-maintenance contracts.

When custom work produces a broadly reusable capability, the reusable part should move back into the product or a maintained module where appropriate.

## 8. What remains in the open baseline

The Community edition should retain the basic reasons engineers adopt TauTerm:

- basic SSH/SFTP connectivity;
- basic Serial connectivity;
- basic TCP/UDP debugging;
- ordinary terminal use;
- access to customer-owned local data;
- practical baseline access to purchased first-party instruments.

Commercial value should focus on deeper analysis, reproducibility, automation, collaboration, governance, industrial depth, deployment and support.

## 9. Go-to-market sequence

### Stage 1 — establish daily use

Focus on Community quality, reliable releases, technical documentation and real embedded/device-engineering workflows.

Signals to watch:

- engineers keep TauTerm open during normal work;
- users actively use more than one workflow domain in the same application;
- external feedback changes product priorities;
- extension points are used outside the core team.

### Stage 2 — validate Professional value

Introduce Professional only when Recording/Replay, Signal Lab, Data Lens, Automation or another advanced workflow creates a clear standalone productivity benefit.

The paid tier should have a positive professional value proposition rather than being defined by removing baseline capabilities from Community.

### Stage 3 — introduce the first instrument

Launch the first Tau instrument when both the hardware and its TauTerm integration meet the same product-quality bar.

The instrument should feel like a native part of the TauTerm Workspace from first connection through capture, decoding, recording and update lifecycle.

### Stage 4 — industrial deployment

Use proven railway/industrial workflows, offline deployment, replayable evidence, instrument correlation and support to validate Industrial packaging and maintenance contracts.

### Stage 5 — team platform

Add deeper collaboration, private registries and governance when real organizations are actively sharing TauTerm Workspaces, decoders, recordings and automation assets.

## 10. Commercial validation gates

Before making a commercial package permanent, validate:

1. **User value:** does the capability save meaningful engineering time or improve evidence/reproducibility?
2. **Packaging clarity:** can a customer understand why the capability belongs in this tier?
3. **Offline viability:** can the intended industrial workflow operate without Internet access?
4. **Supportability:** can the feature be maintained, tested and documented over a long lifecycle?
5. **Data ownership:** can customers continue to access their own data regardless of license state?
6. **Hardware economics:** for instruments, do margin, warranty and support assumptions remain sustainable?

## 11. Compounding commercial value

TauTerm's commercial strength should come from a coherent system of products and services:

- a trusted open-source engineering core;
- local-first workflows suitable for isolated environments;
- professional Recording/Replay, Signal Lab and Data Lens capabilities;
- maintained railway and industrial modules;
- first-party analyzers with deep TauTerm integration;
- an extension ecosystem;
- controlled enterprise deployment and support;
- long-lived engineering data and workflow compatibility.

The objective is a sustainable product family in which software, industrial modules, support and hardware reinforce the same engineering platform.