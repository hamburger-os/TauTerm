import { CSSProperties, ReactNode } from "react";
import styles from "./GlassPanel.module.css";

type GlassSurface = "chrome" | "content" | "accent";

interface GlassPanelProps {
  children: ReactNode;
  className?: string;
  style?: CSSProperties;
  /** 材质层：chrome 可轻量采样背景；content 不使用 backdrop blur；accent 用于小型强调层 */
  surface?: GlassSurface;
  /** 面板变体：默认 / 高亮 */
  variant?: "default" | "elevated";
  /** 内边距大小 */
  padding?: "none" | "sm" | "md" | "lg";
}

const SURFACE_CLASS: Record<GlassSurface, string> = {
  chrome: "liquid-glass",
  content: "liquid-glass-content",
  accent: "liquid-glass-accent",
};

/**
 * TauTerm Liquid Glass 面板。
 *
 * 默认是 chrome 材质；承载终端、文件/数据内容的大面积面板必须显式使用
 * surface="content"，避免把 backdrop-filter 带进内容渲染路径。
 */
export default function GlassPanel({
  children,
  className = "",
  style,
  surface = "chrome",
  variant = "default",
  padding = "md",
}: GlassPanelProps) {
  const classes = [
    SURFACE_CLASS[surface],
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
