import { InputHTMLAttributes, forwardRef } from "react";
import styles from "./GlassInput.module.css";

interface GlassInputProps extends InputHTMLAttributes<HTMLInputElement> {
  /** 标签文本 */
  label?: string;
  /** 错误信息 */
  error?: string;
  /** 是否占满宽度 */
  fullWidth?: boolean;
}

/**
 * 玻璃拟态输入框组件
 * 支持焦点发光动画
 */
const GlassInput = forwardRef<HTMLInputElement, GlassInputProps>(
  ({ label, error, fullWidth = false, className = "", ...props }, ref) => {
    const classes = [
      styles.wrapper,
      fullWidth && styles.fullWidth,
      error && styles.hasError,
      className,
    ]
      .filter(Boolean)
      .join(" ");

    return (
      <div className={classes}>
        {label && <label className={styles.label}>{label}</label>}
        <input ref={ref} className={`${styles.input} liquid-glass-input`} {...props} />
        {error && <span className={styles.error}>{error}</span>}
      </div>
    );
  }
);

GlassInput.displayName = "GlassInput";

export default GlassInput;
