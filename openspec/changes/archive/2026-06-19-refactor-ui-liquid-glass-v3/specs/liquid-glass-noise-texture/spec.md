# liquid-glass-noise-texture

## Purpose

定义 SVG `feTurbulence` 噪点纹理注入系统——通过 CSS `background-image` Data URI 内联 SVG 滤镜，在玻璃面板表面叠加微细噪点，增强磨砂/霜化材质真实感。噪点参数随主题微调。

## ADDED Requirements

### Requirement: SVG Noise Texture Injection
Glass panel elements SHALL include an SVG noise texture as part of their `background-image` property.

The noise SHALL be generated via an inline SVG Data URI using the `feTurbulence` filter with `type="fractalNoise"`, layered above the background gradient.

The noise texture SHALL use a 200x200 viewBox SVG with `stitchTiles="stitch"` for seamless tiling.

#### Scenario: Glass panel renders with noise texture
- **WHEN** a `.liquid-glass` panel is displayed
- **THEN** a subtle noise grain is visible on the glass surface, creating a frosted/matte appearance

#### Scenario: Noise tiles seamlessly
- **WHEN** observing a large glass panel (e.g., 1200x800px)
- **THEN** the noise texture repeats without visible seams or tiling artifacts

### Requirement: Theme-Aware Noise Parameters
The noise texture's `baseFrequency` and overall `opacity` SHALL vary by active theme:

**google-glow**: baseFrequency 0.8, opacity 0.04, numOctaves 3
**obsidian**: baseFrequency 0.85, opacity 0.05, numOctaves 3
**frosted**: baseFrequency 0.9, opacity 0.03, numOctaves 3

#### Scenario: Dark theme noise is more visible
- **WHEN** obsidian theme is active
- **THEN** glass panels show a slightly more pronounced noise grain (opacity 0.05) compared to google-glow (0.04)

#### Scenario: Light theme noise is subtle
- **WHEN** frosted theme is active
- **THEN** glass panels show a very faint noise grain (opacity 0.03) with finer frequency (0.9)

### Requirement: Noise as Background Layer
The noise SVG SHALL be declared as the first layer in `background-image`, followed by the glass fill gradient.

The gradient SHALL repeat after the noise SVG to provide the fill color beneath the semi-transparent noise overlay.

#### Scenario: Noise overlays gradient fill
- **WHEN** a glass panel is displayed
- **THEN** the noise grain appears on top of the glass gradient fill, creating texture depth
