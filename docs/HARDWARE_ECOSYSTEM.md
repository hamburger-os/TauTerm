# TauTerm Hardware Ecosystem Direction

> This document describes the intended software architecture and product experience for future first-party engineering instruments. It is a direction, not a hardware specification or release commitment.

## 1. Goal

TauTerm should become the common upper-computer software for a family of engineering analyzers.

A future CAN analyzer is the first likely example. Additional analyzers may follow when they solve a clear engineering problem and fit the same platform model.

The core idea is:

> **One instrument adds a new source of engineering data; it should not create a new software silo.**

Every first-party instrument should benefit from the same Workspace, Recorder, Unified Timeline, Signal Lab, Data Lens and Automation systems.

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

## 2. Platform principles

### 2.1 TauTerm is the common control and analysis environment

Instrument discovery, setup, capture, monitoring, decoding, recording and automation should happen inside TauTerm where practical.

A small recovery/configuration utility may exist when required for low-level firmware recovery, driver repair or manufacturing, but normal engineering use should remain inside TauTerm.

### 2.2 First-party hardware gets the most integrated experience

First-party instruments should be discoverable, identifiable and usable with minimal configuration.

The architecture can still support third-party or generic adapters through stable extension boundaries. First-party devices should provide the strongest integration through known capabilities, hardware timestamping, firmware lifecycle support, device diagnostics and validated workflows.

### 2.3 Capability negotiation is versioned

TauTerm should discover instrument capabilities rather than hard-code assumptions about a model or hardware revision.

Example capability names:

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

New hardware revisions should be able to add capabilities without forcing unrelated UI or protocol changes.

### 2.4 Raw capture is durable data

The capture path should retain raw frames or samples close enough to the source that a recording can later be replayed, re-decoded or analyzed by newer software.

Rendered tables and charts are views over engineering data, not the only durable representation of that data.

### 2.5 Time is a first-class engineering primitive

Cross-session and cross-instrument analysis depends on trustworthy timestamps.

The integration model should account for:

- host receive timestamp;
- hardware capture timestamp where available;
- timestamp resolution;
- clock source metadata;
- device/host clock offset information;
- clock reset and discontinuity markers;
- future multi-instrument synchronization where hardware supports it.

The Recorder should preserve enough timing metadata for Unified Timeline to distinguish capture time from host receive time.

### 2.6 Instrument workflows are local-first

Capture, decoding, plotting and replay must work offline.

Drivers, firmware packages and calibration metadata needed in isolated environments should have a controlled offline distribution path.

## 3. Software-facing instrument model

Physical instruments and protocol sessions can share common session/event infrastructure without forcing them into exactly the same adapter abstraction.

A future software-facing model can conceptually expose:

```text
InstrumentManifest
├─ product family
├─ model
├─ device identifier
├─ firmware version
├─ host transport
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

The exact API should be designed when the first instrument implementation begins. The important constraint is that instrument data enters the same product-level event pipeline as protocol data.

## 4. Shared data path

```text
Instrument Driver
      ↓
Raw Frame / Sample
      ↓
Timestamp + Source Metadata
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

A single raw capture may therefore support several later views without duplicating acquisition logic.

## 5. Device lifecycle

The instrument platform should treat the complete device lifecycle as part of product quality.

### Discovery and identity

TauTerm should be able to determine, where supported:

- device family and model;
- stable device identifier;
- firmware version;
- hardware revision;
- capabilities;
- channel count/types;
- calibration metadata status.

### Connection state

Instrument sessions should expose meaningful states such as:

- unavailable;
- ready;
- configured;
- capturing;
- paused;
- faulted;
- updating firmware.

### Health and diagnostics

Where hardware supports it, TauTerm should surface:

- transport errors;
- overflow/drop counters;
- device temperature or supply warnings;
- bus/controller errors;
- timestamp discontinuities;
- firmware compatibility issues.

These events should be recordable when they affect engineering interpretation.

## 6. Future CAN analyzer

The first-party CAN analyzer should be designed together with its TauTerm workflow.

### 6.1 Initial software-facing scope to evaluate

- CAN 2.0A / 2.0B;
- CAN FD where the hardware design supports it;
- configurable nominal and data bit rates;
- hardware timestamps;
- receive and transmit/injection workflows;
- acceptance filtering;
- bus status and error visibility;
- trace recording;
- controlled replay/transmit from trace;
- DBC/symbol decoding through Data Lens;
- statistics and bus load;
- trigger/marker integration;
- multi-channel support when provided by hardware;
- firmware update and device diagnostics.

CAN XL can remain a future consideration unless a concrete product requirement justifies it.

### 6.2 TauTerm-native CAN workflows

A CAN session should participate in the same engineering context as other sessions.

Examples:

- correlate a CAN error frame with a Serial console message;
- mark a Recording when a decoded CAN signal crosses a threshold;
- plot decoded CAN signals in Signal Lab;
- trigger an SSH/journald query from a CAN event;
- replay a captured CAN trace together with recorded device/network context;
- compare captures from two test runs;
- run automation against decoded CAN fields.

### 6.3 Safety controls

Transmit, injection and replay features can affect real systems and should have explicit safety boundaries.

Potential controls include:

- clear indication of monitor-only versus transmit-capable mode;
- explicit confirmation before high-impact replay/injection operations;
- rate and loop limits;
- visible active-transmit state;
- automatic stop when the device or host session is closed;
- audit/recording of automated transmit actions where appropriate.

## 7. Multi-instrument direction

Additional instrument categories should be considered only when they have:

1. a clear engineering problem;
2. credible hardware value;
3. a natural fit with the TauTerm data model;
4. useful interaction with Recording, Timeline, Signal Lab, Data Lens or Automation.

Possible categories may include network/protocol analyzers, serial/fieldbus interfaces, mixed digital/analog capture devices or domain-specific railway/industrial instruments.

The platform should avoid one-off instrument designs whose data cannot participate in the shared workflow.

## 8. Hardware/software versioning

Long-lived engineering hardware requires explicit compatibility planning.

Recommended properties:

- versioned host-device protocol;
- backward-compatible capability discovery;
- explicit firmware compatibility ranges;
- recoverable firmware-update path;
- signed firmware for production devices;
- stable capture/event schemas;
- migration/version metadata in recordings;
- clear support and LTS policy for industrial deployments.

## 9. Commercial relationship

A first-party instrument should provide a durable and useful local workflow when purchased.

The intended experience is:

1. connect the instrument to TauTerm;
2. obtain a useful baseline capture/inspection workflow immediately;
3. optionally use Professional/Industrial capabilities for deeper analysis, correlation, automation, reporting and support;
4. add more first-party instruments later without changing the overall engineering environment.

Basic access to purchased hardware should remain available even if a software maintenance period ends.

See [COMMERCIALIZATION.md](COMMERCIALIZATION.md) for packaging principles.

## 10. Compounding platform value

The long-term value of the hardware ecosystem is the shared workflow:

> **A growing family of engineering instruments whose data can be captured, decoded, plotted, recorded, replayed and correlated with remote systems, embedded devices and industrial networks inside one local-first workbench.**

Every new instrument should strengthen the shared platform, and every improvement to the shared platform should increase the usefulness of every instrument.