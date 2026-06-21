import { CSSProperties, ReactNode } from "react";
import styles from "./GlassPanel.module.css";

interface GlassPanelProps {
  children: ReactNode;
  className?: string;
  style?: CSSProperties;
  /** 面板变体：默认 / 高亮 */
  variant?: "default" | "elevated";
  /** 内边距大小 */
  padding?: "none" | "sm" | "md" | "lg";
}

/**
 * 磨砂玻璃面板组件
 * 使用 backdrop-filter: blur() 实现玻璃拟态效果
 */
export default function GlassPanel({
  children,
  className = "",
  style,
  variant = "default",
  padding = "md",
}: GlassPanelProps) {
  const classes = [
    "liquid-glass",
    styles.panel,
    styles[variant],
    styles[`padding-${padding}`],
    className,
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <div className={classes} style={style}>
      {children}
    </div>
  );
}
