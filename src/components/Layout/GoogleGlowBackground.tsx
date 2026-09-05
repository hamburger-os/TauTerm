/**
 * TauTerm shared Google Ambient background.
 *
 * All themes use the same four Google RGB orbs, geometry and trajectories.
 * Theme tokens only compensate perceived intensity on different base colors;
 * performance tiers control motion amplitude/duration and whether animation runs.
 *
 * Rendering stays cheap: pre-softened radial gradients + transform-only motion.
 */
export default function GoogleGlowBackground() {
  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        overflow: "hidden",
        pointerEvents: "none",
        zIndex: 0,
        background: "var(--bg-base)",
      }}
      aria-hidden="true"
    >
      <div
        className="glow-orb glow-orb-blue"
        style={{
          width: "64vw",
          height: "62vh",
          top: "-2%",
          left: "-5%",
        }}
      />

      <div
        className="glow-orb glow-orb-red"
        style={{
          width: "58vw",
          height: "62vh",
          top: "0%",
          right: "-4%",
        }}
      />

      <div
        className="glow-orb glow-orb-yellow"
        style={{
          width: "64vw",
          height: "58vh",
          bottom: "-6%",
          left: "-4%",
        }}
      />

      <div
        className="glow-orb glow-orb-green"
        style={{
          width: "60vw",
          height: "60vh",
          bottom: "-8%",
          right: "-2%",
        }}
      />
    </div>
  );
}
