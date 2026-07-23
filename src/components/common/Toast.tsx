import { useEffect } from "react";
import Icon from "./Icon";
import type { IconName } from "./Icon";
import styles from "./Toast.module.css";

interface ToastProps {
  type: "success" | "error" | "warning" | "info";
  message: string;
  /** 在列表中的位置索引（用于堆叠） */
  index?: number;
  /** 关闭回调 */
  onClose: () => void;
}

/** Toast 图标映射 */
const iconMap: Record<string, IconName> = {
  success: "check-plain",
  error: "cross-circle",
  warning: "warning",
  info: "info",
};

/**
 * Toast 通知组件
 *
 * 显示在右下角的浮动通知，5 秒后自动消失。
 */
export default function Toast({ type, message, index = 0, onClose }: ToastProps) {
  useEffect(() => {
    const timer = setTimeout(onClose, 5000);
    return () => clearTimeout(timer);
  }, [onClose]);

  return (
    <div
      className={`${styles.toast} liquid-glass-float ${styles[type]}`}
      style={{ bottom: `${24 + index * 56}px` }}
    >
      <Icon name={iconMap[type]} size="sm" className={styles.icon} />
      <span className={styles.message}>{message}</span>
      <button className={styles.close} onClick={onClose}>
        <Icon name="close" size="sm" />
      </button>
    </div>
  );
}
