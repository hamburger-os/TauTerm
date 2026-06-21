interface ProgressBarProps {
  /** 进度百分比 0-100 */
  percent: number;
  /** 高度 px，默认 4 */
  height?: number;
  /** 是否为不确定模式（显示动画） */
  indeterminate?: boolean;
  className?: string;
}

/**
 * 可复用进度条组件
 * 渐变填充 + 发光效果，与全局主题一致
 */
export default function ProgressBar({
  percent,
  height = 4,
  indeterminate = false,
  className = "",
}: ProgressBarProps) {
  return (
    <div
      className={className}
      style={{
        height: `${height}px`,
        background: "var(--glass-border)",
        borderRadius: `${height / 2}px`,
        overflow: "hidden",
      }}
    >
      <div
        style={{
          height: "100%",
          width: indeterminate ? "30%" : `${Math.min(100, Math.max(0, percent))}%`,
          background: "var(--accent-gradient)",
          borderRadius: `${height / 2}px`,
          transition: "width 200ms ease",
          boxShadow: "0 0 8px var(--accent-glow)",
          animation: indeterminate
            ? "shimmer 1.5s ease-in-out infinite"
            : undefined,
        }}
      />
    </div>
  );
}
