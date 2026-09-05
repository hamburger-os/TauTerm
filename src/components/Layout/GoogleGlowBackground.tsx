/**
 * TauTerm shared Google Ambient field.
 *
 * Two oversized raster-friendly gradient fields carry the same Google RGB set
 * across every theme. The fields are intentionally larger than the viewport so
 * no radial-gradient edge can read as a visible "orb". Animation is transform-only.
 */
export default function GoogleGlowBackground() {
  return (
    <div className="ambient-root" aria-hidden="true">
      <div className="ambient-field ambient-field-a" />
      <div className="ambient-field ambient-field-b" />
    </div>
  );
}
