/**
 * 动态炫彩流光背景组件
 *
 * 渲染 4 个 Google 四色光球在页面最底层（z-index: 0），
 * 应用 CSS @keyframes morph 不规则形变和 flow 盘旋运动动画。
 * 光球的透明度、模糊半径和颜色混合模式由 CSS 自定义属性控制，
 * 随主题切换自动适配。
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
      {/* 蓝色 — Google Blue #4285F4 */}
      <div
        className="glow-orb"
        style={{
          width: "60vw",
          height: "60vh",
          top: "-10%",
          left: "-10%",
          background: "#4285F4",
          animation:
            "morph 15s ease-in-out infinite both alternate, flow1 25s linear infinite",
        }}
      />

      {/* 红色 — Google Red #EA4335 */}
      <div
        className="glow-orb"
        style={{
          width: "50vw",
          height: "60vh",
          top: "-5%",
          right: "-5%",
          background: "#EA4335",
          animation:
            "morph 18s ease-in-out infinite both alternate-reverse, flow2 28s linear infinite",
        }}
      />

      {/* 黄色 — Google Yellow #FBBC05 */}
      <div
        className="glow-orb"
        style={{
          width: "60vw",
          height: "55vh",
          top: "40%",
          left: "-5%",
          background: "#FBBC05",
          animation:
            "morph 12s ease-in-out infinite both alternate, flow3 22s linear infinite",
        }}
      />

      {/* 绿色 — Google Green #34A853 */}
      <div
        className="glow-orb"
        style={{
          width: "55vw",
          height: "55vh",
          top: "45%",
          right: "0%",
          background: "#34A853",
          animation:
            "morph 16s ease-in-out infinite both alternate-reverse, flow4 26s linear infinite",
        }}
      />
    </div>
  );
}
