# TauTerm Documentation

This directory contains the long-lived product, engineering and release documentation for TauTerm.

The root [README](../README.md) is the product entry point. Documents here should each have one clear responsibility and should distinguish current implementation from future direction.

## Product direction

These documents describe decisions and design direction. Planned capabilities in them are not release commitments.

| Document | Purpose |
|---|---|
| [PRODUCT_STRATEGY.md](PRODUCT_STRATEGY.md) | Product vision, target users, principles, capability pillars, roadmap model and product decision filter |
| [HARDWARE_ECOSYSTEM.md](HARDWARE_ECOSYSTEM.md) | Direction for first-party analyzers, the common upper-computer model, timing/data requirements and future CAN integration |
| [COMMERCIALIZATION.md](COMMERCIALIZATION.md) | Open-core/commercial/hardware business model, packaging hypotheses, licensing principles and validation gates |

## Engineering and development

These documents describe the current codebase, build environment and supported platforms.

| Document | Purpose |
|---|---|
| [ARCHITECTURE.md](ARCHITECTURE.md) | Current microkernel, protocol adapters, session/I/O model and frontend/backend architecture |
| [BUILDING.md](BUILDING.md) | Developer prerequisites and source-build instructions |
| [SUPPORTED_PLATFORMS.md](SUPPORTED_PLATFORMS.md) | Supported operating systems, architectures and packaging/signing status |

When a future product concept becomes an implemented subsystem, its technical contract should be documented in the architecture documentation rather than leaving implementation details only in strategy documents.

## Release documentation

| Document | Purpose |
|---|---|
| [RELEASING.md](RELEASING.md) | Maintainer release process and release-engineering procedure |
| [RELEASE_NOTES_v0.5.0.md](RELEASE_NOTES_v0.5.0.md) | Historical release notes for v0.5.0 |
| [RELEASE_NOTES_v0.5.1.md](RELEASE_NOTES_v0.5.1.md) | Historical release notes for v0.5.1 |

The repository root [CHANGELOG.md](../CHANGELOG.md) remains the canonical chronological record of shipped changes.

## Assets

`assets/` contains durable screenshots and other media referenced by repository documentation. Product documentation should prefer real application output over mockups when describing implemented features.

## Documentation principles

When adding or revising documentation:

1. **One document, one responsibility.** Avoid creating a new file when an existing canonical document already owns the topic.
2. **Describe TauTerm on its own terms.** Explain the problem, design and intended workflow directly; avoid product comparisons and named competitor references.
3. **Separate current state from direction.** Architecture and user documentation must not present planned capabilities as already implemented.
4. **Prefer capability-oriented structure.** Product direction should describe engineering outcomes and shared platform capabilities rather than collecting feature checkboxes.
5. **Keep links explicit.** Related documents should cross-link where the boundary between product, architecture, hardware and commercialization matters.
6. **Keep durable repository knowledge here.** Temporary campaign copy, one-off launch messaging and transient promotional checklists should live outside the long-lived engineering documentation.
7. **Update the index.** New durable documents should be added to this file so the `docs/` tree remains navigable.

## Source of truth

Use the following priority when documents appear to disagree:

1. shipped behavior: current code, tests, [CHANGELOG.md](../CHANGELOG.md) and release artifacts;
2. current technical design: [ARCHITECTURE.md](ARCHITECTURE.md) and engineering documentation;
3. future product direction: [PRODUCT_STRATEGY.md](PRODUCT_STRATEGY.md) and related strategy documents.

A strategy document can guide implementation, but it must not be used as evidence that a capability has already shipped.