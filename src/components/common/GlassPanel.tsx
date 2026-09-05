import { CSSProperties, ReactNode } from "react";
import styles from "./GlassPanel.module.css";

type GlassSurface = "shell" | "content" | "control" | "accent";

interface GlassPanelProps {
  children: ReactNode;
  className?: string;
  style?: CSSProperties;
  /** 材质层：shell 可轻量采样；content 是大面积内容；control 是稳定交互工作台；accent 是小型强调层 */
  surface?: GlassSurface;
  /** 面板变体：默认 / 高亮 */
  variant?: "default" | "elevated";
  /** 内边距大小 */
  padding?: "none" | "sm" | "md" | "lg";
}

const SURFACE_CLASS: Record<GlassSurface, string> = {
  shell: "liquid-glass",
  content: "liquid-glass-content",
  control: "liquid-control-surface",
  accent: "liquid-glass-accent",
};

/**
 * TauTerm Liquid Glass 面板。
 *
 * 默认是 shell 材质；承载终端、文件/数据内容的大面积面板必须显式使用
 * surface="content"，避免把 backdrop-filter 带进内容渲染路径；高密度表单/编辑工具使用
 * surface="control"，在不引入 backdrop blur 的前提下稳定控件对比度。
 */
export default function GlassPanel({
  children,
  className = "",
  style,
  surface = "shell",
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
