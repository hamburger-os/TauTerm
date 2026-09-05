/**
 * 轻量环境流光背景。
 *
 * 视觉保持 Google 四色氛围，但光团已经在 CSS 中使用 radial-gradient 预柔化，
 * 仅执行 transform 动画，不再使用大半径 filter: blur、border-radius morph
 * 或 mix-blend-mode。Compatibility 档自动静态化并减少光团数量。
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
          width: "60vw",
          height: "60vh",
          top: "-10%",
          left: "-10%",
        }}
      />

      <div
        className="glow-orb glow-orb-red"
        style={{
          width: "50vw",
          height: "60vh",
          top: "-5%",
          right: "-5%",
        }}
      />

      <div
        className="glow-orb glow-orb-yellow"
        style={{
          width: "60vw",
          height: "55vh",
          top: "40%",
          left: "-5%",
        }}
      />

      <div
        className="glow-orb glow-orb-green"
        style={{
          width: "55vw",
          height: "55vh",
          top: "45%",
          right: "0%",
        }}
      />
    </div>
  );
}
