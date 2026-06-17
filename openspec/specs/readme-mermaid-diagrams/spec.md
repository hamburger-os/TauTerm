# readme-mermaid-diagrams

## Purpose

The README documentation SHALL use Mermaid diagrams instead of ASCII art diagrams for better rendering on GitHub, easier maintenance, and improved readability.

## Requirements

### Requirement: Architecture Overview Mermaid Diagram
The README SHALL replace the ASCII art architecture overview diagram with a Mermaid `graph TB` diagram that depicts the TauTerm microkernel with its 8 core modules (Window Manager, Tab Host, IPC Bridge, Config Store, Plugin Host, Theme Engine, Shortcut Engine, i18n Engine) inside a subgraph, connected to a Plugin Registry that fans out to 8 protocol plugins (Serial, SSH, Telnet, TCP Raw, TRDP, Shell Local, FTP, iPerf3).

#### Scenario: Architecture diagram renders on GitHub
- **WHEN** a user views README.md on GitHub
- **THEN** the Mermaid architecture diagram renders as an interactive flowchart showing the microkernel, plugin registry, and all 8 protocol plugins

#### Scenario: All core modules are shown
- **WHEN** the diagram is rendered
- **THEN** all 8 kernel modules are visible inside a "TauTerm Microkernel" subgraph

#### Scenario: All protocol plugins are shown
- **WHEN** the diagram is rendered
- **THEN** all 8 protocol plugins (Serial, SSH, Telnet, TCP Raw, TRDP, Shell Local, FTP, iPerf3) are visible under the Plugin Registry

### Requirement: Plugin Lifecycle State Diagram
The README SHALL replace the plain-text lifecycle flow (`Discover → Load → Initialize → Ready → (Stop → Unload)`) with a Mermaid `stateDiagram-v2` diagram showing the complete plugin lifecycle states and transitions.

#### Scenario: Lifecycle states are rendered
- **WHEN** a user views README.md on GitHub
- **THEN** the Mermaid state diagram shows Discover → Load → Initialize → Ready states with optional Stop → Unload transition

#### Scenario: Lifecycle text is removed
- **WHEN** the state diagram is rendered
- **THEN** the original plain-text `Discover → Load → Initialize → Ready → (Stop → Unload)` string is replaced by the Mermaid block

### Requirement: Transfer Subsystem Mermaid Diagram
The README SHALL replace the ASCII art transfer subsystem diagram with a Mermaid `graph TD` diagram showing the TransferManager dispatching to three parallel strategy branches: Inline (Serial with YModem/XModem/ZModem), SideChannel (SSH with SFTP/SCP), and SeparateConnection (FTP).

#### Scenario: Transfer diagram renders on GitHub
- **WHEN** a user views README.md on GitHub
- **THEN** the Mermaid transfer diagram renders showing TransferManager at the top branching into three strategies

#### Scenario: All three strategies are shown with protocols
- **WHEN** the diagram is rendered
- **THEN** Inline (YModem, XModem, ZModem), SideChannel (SFTP, SCP), and SeparateConnection (FTP) strategies are all visible with their associated protocols

### Requirement: Security Model Mermaid Diagram
The README SHALL replace the ASCII art security model diagram with a Mermaid `graph LR` diagram showing the Credential Store with encrypted password, SSH key, and certificate/token storage, the primary backend (OS keyring via keyring-rs), and the AES-256-GCM file fallback.

#### Scenario: Security model diagram renders on GitHub
- **WHEN** a user views README.md on GitHub
- **THEN** the Mermaid security diagram renders showing the Credential Store hierarchy and fallback path

#### Scenario: All credential types are shown
- **WHEN** the diagram is rendered
- **THEN** password, SSH key, and certificate/token credential types are visible

#### Scenario: Primary and fallback backends are shown
- **WHEN** the diagram is rendered
- **THEN** the OS keyring primary backend and AES-256-GCM file fallback are both visible

### Requirement: Non-diagram Content Unchanged
All non-diagram content in the README SHALL remain unchanged, including tables, code blocks, lists, headings, and descriptive text.

#### Scenario: Tables are preserved
- **WHEN** comparing the updated README to the original
- **THEN** all tables (Design Principles, Plugin Capabilities, Content Adapters, Protocol Support Matrix, Tech Stack, Keyboard Shortcuts, Roadmap, Comparison) remain identical in content

#### Scenario: Code blocks are preserved
- **WHEN** comparing the updated README to the original
- **THEN** all code blocks (manifest.json, ProtocolAdapter trait, registerPlugin API, project structure, build commands) remain identical in content
