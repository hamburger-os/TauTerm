# TauTerm Hardware Ecosystem Direction

> This document describes the intended software architecture and product experience for future first-party engineering instruments. It is a direction, not a hardware specification or release commitment.

## Goal

TauTerm should become the common desktop software for a family of engineering analyzers instead of creating a different upper-computer application for every instrument.

A future CAN analyzer is the first likely example. Additional analyzers may follow.

The strategic idea is simple:

> **One instrument adds a new source of engineering data; it should not create a new software silo.**

Every Tau instrument should immediately benefit from the same Workspace, Recorder, Unified Timeline, Signal Lab, Data Lens and Automation systems.

## Why this matters

Traditional instrument products often split engineering context:

- one application for CAN;
- another for serial/network debugging;
- another for signal plotting;
- another for remote Linux logs;
- separate capture formats and automation APIs.

TauTerm can differentiate by correlating those domains in one local-first engineering environment.

Example:

```text
Tau CAN Analyzer ─ CAN/CAN FD ─┐
Serial device ─ UART/RS-485 ───┤
TRDP network ─ Ethernet ────────┼─ TauTerm Workspace
Linux target ─ SSH/journald ────┤     ├─ Recorder / Replay
TCP/UDP service ─ Network ──────┘     ├─ Unified Timeline
                                     ├─ Signal Lab
                                     ├─ Data Lens
                                     └─ Automation
```

The resulting product is more valuable than a collection of independent analyzer applications.

## Product principles

### 1. TauTerm is the control plane

Instrument setup, capture, monitoring, decoding, recording and automation should happen inside TauTerm where practical.

Avoid permanent standalone companion applications unless a low-level recovery/configuration tool is necessary.

### 2. First-party hardware gets the best experience, not the only experience

Tau hardware should be discoverable, identifiable and usable with minimal configuration.

At the same time, the architecture should not unnecessarily prevent third-party hardware adapters. A healthy software ecosystem increases TauTerm adoption, while first-party instruments differentiate through integration quality, reliability, hardware timestamping, supported capabilities and official maintenance.

### 3. Capability negotiation, not hard-coded product assumptions

TauTerm should learn what an attached instrument can do through a versioned capability model.

Potential capability examples:

```text
capture.can
capture.can_fd
capture.analog
capture.digital
hardware_timestamp
trigger.hardware
stream.realtime
firmware_update
calibration_info
output.inject
```

New hardware revisions should be able to add capabilities without requiring unrelated UI rewrites.

### 4. Raw data remains valuable

Capture pipelines should retain raw frames/samples close enough to the source that recordings can be replayed, re-decoded or analyzed with newer software.

Do not make the rendered table/chart the only durable representation of a capture.

### 5. Time is a first-class engineering primitive

Cross-instrument debugging depends on trustworthy timestamps.

The instrument integration model should plan for:

- host receive timestamp;
- hardware capture timestamp where available;
- timestamp resolution and clock source metadata;
- device/host clock offset information;
- clock reset/discontinuity markers;
- future multi-instrument synchronization where hardware supports it.

This is essential for Unified Timeline correlation.

### 6. Local-first operation

Instrument capture and analysis must work offline. Firmware packages, drivers and calibration metadata needed in isolated environments should have an enterprise-friendly offline distribution path.

## Suggested software-facing instrument model

A future instrument adapter can conceptually expose:

```text
InstrumentManifest
├─ id / model / serial number
├─ firmware version
├─ transport
├─ capabilities
├─ channel definitions
├─ clock information
└─ configuration schema

InstrumentSession
├─ configure()
├─ start_capture()
├─ stop_capture()
├─ send/inject()          # when supported
├─ events / frames / samples
├─ health/status
└─ firmware/update hooks
```

This does not require the existing `ProtocolAdapter` API to be stretched beyond its intended purpose. The implementation can share common session/event infrastructure while keeping protocol connections and physical instruments as separate architectural concepts where appropriate.

## Common data path

Instrument events should enter the same product-level data pipeline used by protocol sessions:

```text
Instrument Driver
      ↓
Raw Frame / Sample
      ↓
Framing / Decoder
      ↓
Structured Event / Signal
   ├─ Native instrument view
   ├─ Signal Lab
   ├─ Data Lens
   ├─ Recorder / Replay
   ├─ Unified Timeline
   └─ Automation
```

This shared path is the main reason to use TauTerm as the upper computer for multiple instruments.

## Future CAN analyzer

The first-party CAN analyzer should be designed together with the TauTerm workflow instead of treating the desktop application as an afterthought.

### Software-facing baseline

Likely baseline areas to consider when the hardware project begins:

- CAN 2.0A / 2.0B;
- CAN FD if hardware economics permit;
- configurable nominal/data bit rates;
- hardware timestamps;
- receive and transmit/injection workflows;
- acceptance filtering;
- bus status/error visibility;
- trace recording;
- replay/transmit from trace with explicit safety controls;
- DBC/symbol decoding as a Data Lens source;
- statistics and bus load;
- trigger/marker integration;
- multi-channel support if hardware provides it;
- firmware update and device diagnostics.

CAN XL can remain a future consideration rather than an initial requirement unless target customers create a clear need.

### TauTerm-native workflows

A CAN session should not stop at a frame table.

Useful integrated workflows include:

- correlate a CAN fault frame with a Serial console message;
- mark a Recording when a CAN signal crosses a threshold;
- plot decoded CAN signals in Signal Lab;
- trigger SSH/journald collection from a CAN event;
- replay a captured CAN trace together with recorded device/network context;
- compare captures from two test runs;
- run automation against decoded CAN fields.

These cross-domain workflows are a stronger differentiator than matching every checkbox of a standalone CAN utility on day one.

## Multi-instrument future

Possible future instruments should be evaluated by whether they strengthen the same engineering model.

Examples could include network/protocol analyzers, serial/fieldbus interfaces, mixed digital/analog capture devices or domain-specific railway/industrial tools.

Do not commit to a hardware category simply because TauTerm can display its data. A new instrument should have a clear user problem, credible hardware differentiation and strong integration value.

## Hardware/software versioning

Instrument support should plan early for long product lifetimes.

Recommended properties:

- versioned host-device protocol;
- backward-compatible capability discovery;
- explicit firmware compatibility ranges;
- recoverable firmware-update path;
- signed firmware for production devices;
- stable capture file/event schemas;
- migration/version metadata in recordings;
- clear support/LTS policy for industrial customers.

## Commercial relationship

First-party instruments are both products and distribution channels for TauTerm.

The desired customer experience:

1. buy a Tau instrument;
2. connect it to TauTerm and obtain a useful local workflow immediately;
3. optionally buy Professional/Industrial capabilities for deeper analysis, correlation, automation, reporting and support;
4. add another Tau instrument later and gain more value from the same Workspace rather than another standalone application.

Basic access to purchased hardware should not disappear when a software subscription or maintenance period ends.

See [COMMERCIALIZATION.md](COMMERCIALIZATION.md) for packaging principles.

## Competitive advantage

The target moat is not simply “we make a CAN analyzer.”

It is:

> **A growing family of engineering instruments whose data can be connected, decoded, plotted, recorded, replayed and correlated with servers, embedded devices and industrial networks inside one local-first workbench.**

That creates a compounding software/hardware advantage: every new instrument strengthens TauTerm, and every TauTerm capability increases the value of every instrument.