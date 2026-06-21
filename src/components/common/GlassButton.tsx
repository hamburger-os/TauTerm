import { ButtonHTMLAttributes, ReactNode } from "react";
import styles from "./GlassButton.module.css";

interface GlassButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  children: ReactNode;
  /** 按钮变体 */
  variant?: "primary" | "secondary" | "ghost" | "danger";
  /** 按钮大小 */
  size?: "sm" | "md" | "lg";
  /** 是否占满宽度 */
  fullWidth?: boolean;
  /** 加载状态 */
  loading?: boolean;
}

/**
 * 玻璃拟态按钮组件
 * 支持强调渐变、悬停发光、按压激活状态
 */
export default function GlassButton({
  children,
  variant = "secondary",
  size = "md",
  fullWidth = false,
  loading = false,
  className = "",
  disabled,
  ...props
}: GlassButtonProps) {
  const globalClass = variant === "primary" ? "liquid-primary-button" : "liquid-glass-button";
  const classes = [
    styles.button,
    styles[variant],
    styles[size],
    globalClass,
    fullWidth && styles.fullWidth,
    loading && styles.loading,
    className,
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <button
      className={classes}
      disabled={disabled || loading}
      {...props}
    >
      <span className={styles.content}>{children}</span>
    </button>
  );
}
