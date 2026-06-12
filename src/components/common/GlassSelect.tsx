import { SelectHTMLAttributes, forwardRef } from "react";
import styles from "./GlassInput.module.css";

interface GlassSelectProps extends SelectHTMLAttributes<HTMLSelectElement> {
  /** 标签文本 */
  label?: string;
  /** 选项列表 */
  options: { value: string; label: string }[];
  /** 是否占满宽度 */
  fullWidth?: boolean;
}

/**
 * 玻璃拟态下拉选择组件
 * 支持焦点发光动画
 */
const GlassSelect = forwardRef<HTMLSelectElement, GlassSelectProps>(
  ({ label, options, fullWidth = false, className = "", ...props }, ref) => {
    const classes = [
      styles.wrapper,
      fullWidth && styles.fullWidth,
      className,
    ]
      .filter(Boolean)
      .join(" ");

    return (
      <div className={classes}>
        {label && <label className={styles.label}>{label}</label>}
        <select
          ref={ref}
          className={`${styles.input} ${styles.select}`}
          {...props}
        >
          {options.map((opt) => (
            <option key={opt.value} value={opt.value}>
              {opt.label}
            </option>
          ))}
        </select>
      </div>
    );
  }
);

GlassSelect.displayName = "GlassSelect";

export default GlassSelect;
