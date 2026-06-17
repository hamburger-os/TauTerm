## Why

The README currently uses ASCII art box-drawing characters for architectural diagrams. These are hard to maintain, don't render well on all platforms, and aren't interactive. Converting to Mermaid diagrams will make them render natively on GitHub/GitLab, be easier to edit, and support future theming.

## What Changes

- Replace the ASCII art **Architecture Overview** diagram with a Mermaid flowchart showing the microkernel with 8 core modules and plugin registry
- Replace the ASCII art **Transfer Subsystem** diagram with a Mermaid flowchart showing the TransferManager and three strategy branches
- Replace the ASCII art **Security Model** diagram with a Mermaid flowchart showing the Credential Store hierarchy
- Replace the plain-text **Plugin Lifecycle** flow with a Mermaid state diagram
- Keep all other content (tables, code blocks, lists) unchanged

## Capabilities

### New Capabilities
- `readme-mermaid-diagrams`: Convert all ASCII art diagrams in the README to Mermaid syntax, ensuring they render correctly on GitHub and remain semantically equivalent to the originals.

### Modified Capabilities
<!-- No existing specs are modified by this change -->

## Impact

- Affected file: `README.md` (diagram sections only)
- No code, API, or dependency changes
- Purely a documentation improvement
