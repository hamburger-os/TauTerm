import { useEffect, useRef } from "react";
import { createPortal } from "react-dom";
import { motion, AnimatePresence } from "framer-motion";
import type { ContextMenuState } from "../../hooks/useContextMenu";
import Icon from "./Icon";
import type { IconName } from "./Icon";
import styles from "./ContextMenu.module.css";

export interface ContextMenuItem {
  id: string;
  label: string;
  icon?: IconName;
  danger?: boolean;
  disabled?: boolean;
  type?: "item" | "separator";
}

interface ContextMenuProps {
  state: ContextMenuState;
  items: ContextMenuItem[];
  onSelect: (itemId: string) => void;
  onClose: () => void;
  header?: { icon?: string; label: string } | null;
}

/**
 * 右键上下文菜单
 *
 * 使用 createPortal 渲染到 document.body，避免被侧栏 overflow 裁剪。
 * 自动检测屏幕边界调整位置。
 */
export default function ContextMenu({ state, items, onSelect, onClose, header }: ContextMenuProps) {
  const menuRef = useRef<HTMLDivElement>(null);

  // 调整位置避免溢出屏幕
  useEffect(() => {
    if (!state.visible || !menuRef.current) return;
    const rect = menuRef.current.getBoundingClientRect();
    const vw = window.innerWidth;
    const vh = window.innerHeight;

    let adjustedX = state.x;
    let adjustedY = state.y;

    if (rect.right > vw) adjustedX = vw - rect.width - 8;
    if (rect.bottom > vh) adjustedY = vh - rect.height - 8;
    if (adjustedX < 0) adjustedX = 8;

    if (adjustedX !== state.x || adjustedY !== state.y) {
      menuRef.current.style.left = `${adjustedX}px`;
      menuRef.current.style.top = `${adjustedY}px`;
    }
  }, [state]);

  // 点击外部关闭
  useEffect(() => {
    if (!state.visible) return;
    const handler = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        onClose();
      }
    };
    const timer = setTimeout(() => {
      document.addEventListener("click", handler);
    }, 0);
    return () => {
      clearTimeout(timer);
      document.removeEventListener("click", handler);
    };
  }, [state.visible, onClose]);

  // Escape 关闭
  useEffect(() => {
    if (!state.visible) return;
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, [state.visible, onClose]);

  return createPortal(
    <AnimatePresence>
      {state.visible && (
        <motion.div
          ref={menuRef}
          className={styles.menu}
          style={{ left: state.x, top: state.y }}
          initial={{ opacity: 0, scale: 0.92 }}
          animate={{ opacity: 1, scale: 1 }}
          exit={{ opacity: 0, scale: 0.92 }}
          transition={{ duration: 0.12 }}
          onClick={(e) => e.stopPropagation()}
        >
          {/* Header */}
          {header && (
            <div className={styles.header}>
              {header.icon && <span className={styles.headerIcon}>{header.icon}</span>}
              <span className={styles.headerLabel}>{header.label}</span>
            </div>
          )}
          {items.map(item => {
            if (item.type === "separator") {
              return <div key={item.id} className={styles.separator} />;
            }
            return (
              <button
                key={item.id}
                className={`${styles.menuItem} ${item.danger ? styles.danger : ""} ${item.disabled ? styles.disabled : ""}`}
                onClick={() => {
                  if (!item.disabled) {
                    // 先关闭菜单再执行回调：原生文件对话框（open/save）会同步阻塞
                    // JS 线程，如果先 onSelect 后 onClose，React 来不及渲染隐藏菜单
                    onClose();
                    setTimeout(() => onSelect(item.id), 0);
                  }
                }}
                disabled={item.disabled}
              >
                {item.icon && <Icon name={item.icon} size="sm" className={styles.itemIcon} />}
                <span className={styles.itemLabel}>{item.label}</span>
              </button>
            );
          })}
        </motion.div>
      )}
    </AnimatePresence>,
    document.body
  );
}
