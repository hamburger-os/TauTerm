import { useEffect, useId, useRef, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { motion, AnimatePresence, useReducedMotion } from "framer-motion";
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
  header?: { icon?: ReactNode; label: string } | null;
}

/**
 * 右键上下文菜单
 *
 * 使用 createPortal 渲染到 document.body，避免被侧栏 overflow 裁剪。
 * 自动检测屏幕边界调整位置。
 */
export default function ContextMenu({ state, items, onSelect, onClose, header }: ContextMenuProps) {
  const menuRef = useRef<HTMLDivElement>(null);
  const previousFocusRef = useRef<HTMLElement | null>(null);
  const headerId = useId();
  const reducedMotion = useReducedMotion();

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
    if (adjustedY < 0) adjustedY = 8;

    if (adjustedX !== state.x || adjustedY !== state.y) {
      menuRef.current.style.left = `${adjustedX}px`;
      menuRef.current.style.top = `${adjustedY}px`;
    }
  }, [state.visible, state.x, state.y]);

  // 点击外部关闭
  useEffect(() => {
    const handler = (e: PointerEvent) => {
      if (!state.visible) return;
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        onClose();
      }
    };
    // pointerdown + capture: 在任何子元素（含 xterm.js canvas/textarea）拦截之前捕获
    document.addEventListener("pointerdown", handler, true);
    return () => {
      document.removeEventListener("pointerdown", handler, true);
    };
  }, [state.visible, onClose]);

  // Preserve the control that owned focus before the menu opened. Position
  // changes while a menu is already visible (right-clicking another target) must
  // not overwrite this restore target with a menu item that will soon unmount.
  useEffect(() => {
    if (!state.visible) return;

    previousFocusRef.current = document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null;

    return () => {
      const previousFocus = previousFocusRef.current;
      previousFocusRef.current = null;
      if (
        previousFocus
        && previousFocus !== document.body
        && previousFocus.isConnected
      ) {
        requestAnimationFrame(() => previousFocus.focus());
      }
    };
  }, [state.visible]);

  // Focus the first enabled action whenever a fresh context-click repositions an
  // already-open menu, not only on the initial visible=false -> true transition.
  useEffect(() => {
    if (!state.visible) return;
    const frame = requestAnimationFrame(() => {
      const firstEnabled = menuRef.current
        ?.querySelector<HTMLButtonElement>('button[role="menuitem"]:not(:disabled)');
      (firstEnabled ?? menuRef.current)?.focus();
    });
    return () => cancelAnimationFrame(frame);
  }, [state.visible, state.x, state.y]);

  const handleMenuKeyDown = (event: React.KeyboardEvent<HTMLDivElement>) => {
    const menu = menuRef.current;
    if (!menu) return;

    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      onClose();
      return;
    }

    if (event.key === "Tab") {
      event.preventDefault();
      event.stopPropagation();
      onClose();
      return;
    }

    const enabledItems = Array.from(
      menu.querySelectorAll<HTMLButtonElement>('button[role="menuitem"]:not(:disabled)'),
    );
    if (enabledItems.length === 0) return;

    const activeIndex = enabledItems.indexOf(document.activeElement as HTMLButtonElement);
    let nextIndex: number | null = null;

    switch (event.key) {
      case "ArrowDown":
        nextIndex = activeIndex < 0 ? 0 : (activeIndex + 1) % enabledItems.length;
        break;
      case "ArrowUp":
        nextIndex = activeIndex < 0
          ? enabledItems.length - 1
          : (activeIndex - 1 + enabledItems.length) % enabledItems.length;
        break;
      case "Home":
        nextIndex = 0;
        break;
      case "End":
        nextIndex = enabledItems.length - 1;
        break;
      default:
        return;
    }

    event.preventDefault();
    event.stopPropagation();
    if (nextIndex !== null) enabledItems[nextIndex].focus();
  };

  return createPortal(
    <AnimatePresence>
      {state.visible && (
        <motion.div
          ref={menuRef}
          className={`${styles.menu} liquid-glass-float`}
          style={{ left: state.x, top: state.y }}
          role="menu"
          aria-labelledby={header ? headerId : undefined}
          tabIndex={-1}
          initial={reducedMotion ? false : { opacity: 0, scale: 0.92 }}
          animate={reducedMotion ? { opacity: 1 } : { opacity: 1, scale: 1 }}
          exit={reducedMotion ? { opacity: 0 } : { opacity: 0, scale: 0.92 }}
          transition={{ duration: reducedMotion ? 0 : 0.12 }}
          onKeyDown={handleMenuKeyDown}
          onClick={(e) => e.stopPropagation()}
        >
          {/* Header */}
          {header && (
            <div className={styles.header} id={headerId}>
              {header.icon && <span className={styles.headerIcon} aria-hidden="true">{header.icon}</span>}
              <span className={styles.headerLabel}>{header.label}</span>
            </div>
          )}
          {items.map(item => {
            if (item.type === "separator") {
              return <div key={item.id} className={styles.separator} role="separator" />;
            }
            return (
              <button
                key={item.id}
                type="button"
                role="menuitem"
                tabIndex={-1}
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
