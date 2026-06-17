## 1. Architecture Overview Diagram

- [x] 1.1 Replace the ASCII art microkernel architecture diagram (lines 11-38) with a Mermaid `graph TB` block in README.md, using subgraphs for "TauTerm Microkernel" (8 core modules) and a Plugin Registry fan-out to 8 protocol plugins

## 2. Plugin Lifecycle Diagram

- [x] 2.1 Replace the plain-text lifecycle flow `Discover → Load → Initialize → Ready → (Stop → Unload)` (lines 123-125) with a Mermaid `stateDiagram-v2` block showing all 6 states and transitions

## 3. Transfer Subsystem Diagram

- [x] 3.1 Replace the ASCII art transfer subsystem diagram (lines 145-161) with a Mermaid `graph TD` block showing TransferManager dispatching to Inline, SideChannel, and SeparateConnection strategies with their associated protocols

## 4. Security Model Diagram

- [x] 4.1 Replace the ASCII art security model diagram (lines 181-194) with a Mermaid `graph LR` block showing the Credential Store hierarchy, OS keyring backend, and AES-256-GCM fallback

## 5. Verification

- [x] 5.1 Verify all 4 Mermaid diagrams render correctly on GitHub by checking the raw markdown has valid Mermaid syntax
- [x] 5.2 Confirm all non-diagram content (tables, code blocks, lists, text) remains unchanged from the original README
