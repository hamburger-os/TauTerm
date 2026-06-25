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
}

interface ContextMenuProps {
  state: ContextMenuState;
  items: ContextMenuItem[];
  onSelect: (itemId: string, sessionId: string) => void;
  onClose: () => void;
}

/**
 * 右键上下文菜单
 *
 * 使用 createPortal 渲染到 document.body，避免被侧栏 overflow 裁剪。
 * 自动检测屏幕边界调整位置。
 */
export default function ContextMenu({ state, items, onSelect, onClose }: ContextMenuProps) {
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
          {items.map(item => (
            <button
              key={item.id}
              className={`${styles.menuItem} ${item.danger ? styles.danger : ""} ${item.disabled ? styles.disabled : ""}`}
              onClick={() => {
                if (!item.disabled && state.session) {
                  onSelect(item.id, state.session.id);
                  onClose();
                }
              }}
              disabled={item.disabled}
            >
              {item.icon && <Icon name={item.icon} size="sm" className={styles.itemIcon} />}
              <span className={styles.itemLabel}>{item.label}</span>
            </button>
          ))}
        </motion.div>
      )}
    </AnimatePresence>,
    document.body
  );
}
