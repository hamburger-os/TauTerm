import styles from "./ProgressBar.module.css";

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
      className={`${className} ${styles.track}`}
      style={{
        height: `${height}px`,
        borderRadius: `${height / 2}px`,
      }}
    >
      <div
        className={`${styles.fill} ${indeterminate ? styles.indeterminate : ""}`}
        style={{
          width: indeterminate
            ? undefined
            : `${Math.min(100, Math.max(0, percent))}%`,
          borderRadius: `${height / 2}px`,
        }}
      />
    </div>
  );
}
