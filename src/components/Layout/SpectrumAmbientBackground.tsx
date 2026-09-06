/**
 * TauTerm shared four-color ambient field.
 *
 * Two oversized raster-friendly gradient fields carry the same four-color spectrum
 * across every theme. The fields are intentionally larger than the viewport so
 * no radial-gradient edge reads as a visible hard boundary. Animation is transform-only.
 */
export default function SpectrumAmbientBackground() {
  return (
    <div className="ambient-root" aria-hidden="true">
      <div className="ambient-field ambient-field-a" />
      <div className="ambient-field ambient-field-b" />
    </div>
  );
}
