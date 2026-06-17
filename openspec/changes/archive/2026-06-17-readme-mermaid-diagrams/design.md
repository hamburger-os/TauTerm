## Context

The README currently has 4 ASCII art diagrams: Architecture Overview, Plugin Lifecycle, Transfer Subsystem, and Security Model. Converting them to Mermaid provides native rendering on GitHub, easier maintenance, and better readability. The project uses GitHub as its primary code host, which has native Mermaid support.

## Goals / Non-Goals

**Goals:**
- Replace all 4 ASCII art diagrams with semantically equivalent Mermaid diagrams
- Use appropriate Mermaid diagram types for each diagram's purpose
- Preserve all architectural information conveyed by the originals
- Diagrams must render correctly on GitHub

**Non-Goals:**
- Do not change any non-diagram content in the README
- Do not add new diagrams or architectural documentation
- Do not modify the project structure tree (it's a tree, not a diagram)

## Decisions

### Diagram Type Selection

| Original Diagram | Mermaid Type | Rationale |
|---|---|---|
| Architecture Overview (microkernel + plugins) | `graph TB` (top-down flowchart) | Hierarchical system architecture with modules and connections |
| Plugin Lifecycle | `stateDiagram-v2` | Represents a state machine flow: Discover → Load → Initialize → Ready → Stop → Unload |
| Transfer Subsystem | `graph TD` (top-down flowchart) | Strategy dispatch with three parallel branches |
| Security Model (Credential Store) | `graph LR` (left-right flowchart) | Multi-column layout showing store hierarchy and fallback |

### Mermaid Syntax Constraints

- Use `graph` (not `flowchart`) for maximum compatibility with older Mermaid renderers
- Use subgraphs to represent modules/containers like the current box-drawing approach
- Keep node labels consistent with existing text descriptions
- Use `stateDiagram-v2` for the lifecycle to properly represent state transitions

### Styling

- No custom CSS/theme — rely on default Mermaid rendering
- Use standard node shapes (rectangle for processes, rounded for plugins/states)

## Risks / Trade-offs

- **Risk**: GitHub's Mermaid renderer may not support all syntax → **Mitigation**: Use well-established Mermaid syntax (graph/subgraph/stateDiagram-v2), avoid bleeding-edge features
- **Risk**: Mermaid diagrams may look different from ASCII art → **Mitigation**: Acceptable trade-off; Mermaid is more readable and maintainable
- **Risk**: Some text layout may wrap differently → **Mitigation**: Keep node labels concise, use line breaks where needed
