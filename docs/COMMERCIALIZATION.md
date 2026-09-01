# TauTerm Commercialization Strategy

> This document is a product/business hypothesis, not a published price list. Pricing, packaging and licensing should be validated with real users and customers before being treated as commitments.

## Commercial objective

TauTerm should become financially sustainable without weakening the open-source product that earns developer trust.

The business should monetize the points where TauTerm creates professional and organizational value:

- advanced engineering analysis and reproducibility;
- official first-party instruments and instrument integrations;
- specialist industrial capabilities;
- team workflows and governance;
- controlled offline deployment and licensing;
- support, maintenance and long-term releases.

The business should **not** depend on crippling basic SSH, Serial or TCP/UDP workflows in the Community edition.

## Product/business model

TauTerm should use an **open-core + commercial value layer + hardware** model.

The current Community/Core code can remain MIT OR Apache-2.0. Proprietary commercial modules should be kept behind clean package/repository/plugin boundaries so the licensing model is technically and legally understandable.

Because permissive open-source licensing allows commercial forks, the durable commercial moat must come from the TauTerm brand, execution quality, official modules, hardware integration, support, distribution and the compounding engineering workflow rather than from the core source license alone.

## Packaging

### TauTerm Community

**Purpose:** adoption, trust, ecosystem growth and a genuinely useful engineering tool.

Expected to remain free/open source and include strong baseline capabilities such as:

- SSH/SFTP;
- Serial;
- TCP/UDP network debugging;
- TFTP/Telnet/iPerf;
- Local Shell when available;
- basic Workspace/session management;
- Lua scripting and extension/plugin foundations;
- useful baseline recording and data inspection;
- support for official instrument connectivity at a practical baseline level.

Community should be good enough that an engineer can choose TauTerm as a daily tool without buying anything.

### TauTerm Professional

**Purpose:** individual engineers whose productivity or debugging quality justifies paying for advanced workflows.

Potential paid value:

- advanced structured Recording / Replay;
- synchronized multi-session Unified Timeline;
- advanced Signal Lab features and analysis;
- advanced Data Lens tooling and decoder authoring UX;
- richer report/export workflows;
- large/long-duration local recording management;
- advanced Tau Flow automation;
- selected official professional protocol/industry modules;
- optional local/BYOK AI workflow features;
- priority release channel or individual support benefits.

Initial pricing hypotheses to test rather than commit to:

- annual license in roughly the USD 79–129 range;
- perpetual individual license in roughly the USD 249–399 range, including a defined update period;
- regional pricing can be tested for China and other markets.

A perpetual option is strategically important for engineering users who dislike runtime subscriptions.

### TauTerm Industrial / Team

**Purpose:** railway, industrial, device-development and test organizations.

This tier should solve organizational procurement and governance problems rather than merely unlock more buttons.

Potential capabilities:

- shared Workspaces;
- shared decoder/automation libraries;
- recording review, annotations and evidence packages;
- private plugin/instrument registry;
- role/policy controls;
- centralized or enterprise-managed secrets where appropriate;
- audit trails;
- controlled update channels;
- offline activation;
- floating/concurrent licensing;
- lab/site licenses;
- LTS versions and maintenance windows;
- enterprise support and deployment assistance.

Recommended commercial structure:

- perpetual use of the purchased major/product version;
- 12 months of upgrades/support included;
- optional annual maintenance renewal for new versions and support;
- no need for an Internet connection during normal engineering operation.

This matches industrial buying expectations better than a cloud-only SaaS subscription.

### Test Bench / Factory licensing

Where TauTerm becomes part of a fixed workstation, validation rack, HIL bench or production/test station, licensing by named user may be the wrong unit.

Offer a **Test Bench / Station** license that is tied to the engineering workstation or controlled test station and can operate offline.

Potential future license units:

- named engineer;
- floating/concurrent engineer;
- test bench/station;
- laboratory/site;
- enterprise agreement.

## Hardware business

First-party analyzers can become a second major revenue engine and also strengthen software differentiation.

A future CAN analyzer is a natural first candidate. Other analyzers can later join the same software platform.

The desired model is:

1. hardware is profitable on its own;
2. TauTerm Community provides a useful out-of-box experience with the hardware;
3. Professional/Industrial software increases the value of the hardware through advanced recording, decoding, correlation, automation and reporting;
4. owning multiple Tau instruments increases the value of a single TauTerm Workspace rather than forcing customers to install separate applications.

Do not make first-party hardware artificially unusable without a subscription. A customer who buys an instrument should receive a durable, useful local workflow.

Possible bundle structures to validate later:

- instrument + Community software;
- instrument + 1 year Professional entitlement;
- industrial instrument kit + Industrial maintenance/support;
- multi-instrument lab bundle;
- test-bench bundle with offline station license.

Hardware pricing and margin targets should only be set after BOM, certification, support, channel and warranty costs are understood.

## Industry packs and official modules

Commercial expansion can also come from deep, maintained vertical capabilities rather than generic protocol checkboxes.

Examples:

### Railway Pack

Potential value:

- advanced TRDP analysis;
- dataset/configuration tooling;
- cycle/jitter/loss diagnostics;
- railway-oriented reports;
- validated decoder libraries;
- long-term supported releases.

### Industrial Pack

Only add protocols where TauTerm can provide meaningful workflow depth. Candidates may eventually include fieldbus/device protocols that integrate naturally with Data Lens, Recorder and Automation.

### Instrument Packs

Certain advanced first-party instrument capabilities, calibrated analysis modules or specialist decoders can be packaged commercially while basic connectivity remains available.

## Services and support

Services should accelerate product adoption without turning TauTerm into a consulting-only business.

Potential paid services:

- enterprise deployment assistance;
- custom protocol/decoder development;
- custom instrument integration;
- railway/industrial workflow integration;
- training;
- priority support;
- LTS/security maintenance contracts.

Whenever a custom implementation is reusable, prefer turning the generic part into a product capability rather than maintaining permanent customer-specific forks.

## Local-first licensing rules

Commercial licensing must respect the product's industrial positioning.

Preferred properties:

- no mandatory login for core engineering use;
- offline activation path for Industrial customers;
- grace periods that do not interrupt active field/test work;
- purchased perpetual versions keep working after maintenance expires;
- license checks never block access to customer-owned recordings/data;
- enterprise deployments can pin a supported release;
- telemetry is opt-in and must not be required for licensing.

## What not to monetize

Avoid paywalls around the basic reasons people adopt TauTerm:

- basic SSH/SFTP connectivity;
- basic Serial connectivity;
- basic TCP/UDP debugging;
- ordinary terminal use;
- reading a customer's own local data;
- basic access to a purchased Tau instrument.

Charge for professional leverage: deeper analysis, reproducibility, automation, collaboration, governance, supported industry depth and first-party product quality.

## Go-to-market sequence

### Stage 1 — earn trust

Focus on Community quality, releases, technical content and real embedded/device-engineering users.

Success signals:

- users keep TauTerm open as a daily tool;
- users replace multiple utilities with TauTerm;
- external feedback changes the roadmap;
- contributors build against extension points.

### Stage 2 — prove professional willingness to pay

Introduce Professional only after Recording/Replay, Signal Lab, Data Lens or another clearly differentiated workflow is valuable enough to stand on its own.

Do not launch a paid tier whose main message is simply “more protocols.”

### Stage 3 — hardware flywheel

Launch the first Tau instrument when hardware quality and the TauTerm integration are both good enough to feel like one product.

The instrument should become a discovery channel for TauTerm, while TauTerm should make the instrument more valuable than a standalone analyzer.

### Stage 4 — industrial sales

Use real railway/industrial workflows such as TRDP, offline deployment, replayable evidence and instrument correlation to sell Industrial licenses, support and maintenance.

### Stage 5 — team platform

Add collaboration/private registries/governance when there is evidence that multiple engineers inside the same organization actively share TauTerm assets.

## Commercial moat

The intended business moat is the combination of:

- a trusted open-source engineering core;
- a local-first workflow suitable for industrial environments;
- professional Recording/Replay, Signal Lab and Data Lens capabilities;
- deep railway/industrial expertise;
- first-party analyzers with best-in-class TauTerm integration;
- an extension ecosystem;
- enterprise deployment/support;
- the TauTerm brand and accumulated engineering workflows.

Any single software feature can be copied. A coherent software/hardware engineering ecosystem is much harder to replace.