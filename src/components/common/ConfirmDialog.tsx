import { type ReactNode, useEffect, useId, useRef } from "react";
import { createPortal } from "react-dom";
import { AnimatePresence, motion, useReducedMotion } from "framer-motion";
import { useTranslation } from "react-i18next";
import GlassButton from "./GlassButton";
import Icon from "./Icon";
import styles from "./ConfirmDialog.module.css";

export type ConfirmDialogIntent = "primary" | "danger";
export type ConfirmDialogSize = "compact" | "default";

interface ConfirmDialogProps {
  open: boolean;
  title: ReactNode;
  message?: ReactNode;
  children?: ReactNode;
  intent?: ConfirmDialogIntent;
  size?: ConfirmDialogSize;
  busy?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}

/**
 * TauTerm 两按钮确认框的唯一公共实现。
 *
 * 文案合同固定为“取消 / 确认”（由 common.cancel / common.confirm 国际化），
 * 调用方只提供要确认的业务内容与危险级别，避免各模块再次产生不同的
 * Delete / Continue / Paste Anyway 等确认动作文案和不同的弹窗壳实现。
 */
export default function ConfirmDialog({
  open,
  title,
  message,
  children,
  intent = "primary",
  size = "default",
  busy = false,
  onConfirm,
  onCancel,
}: ConfirmDialogProps) {
  const { t } = useTranslation();
  const dialogRef = useRef<HTMLDivElement>(null);
  const previouslyFocusedRef = useRef<HTMLElement | null>(null);
  const reducedMotion = useReducedMotion();
  const id = useId().replace(/:/g, "");
  const titleId = `confirm-dialog-title-${id}`;
  const messageId = `confirm-dialog-message-${id}`;

  useEffect(() => {
    if (!open) return;

    previouslyFocusedRef.current =
      document.activeElement instanceof HTMLElement ? document.activeElement : null;

    const frame = requestAnimationFrame(() => {
      dialogRef.current
        ?.querySelector<HTMLButtonElement>('[data-action="cancel"]')
        ?.focus();
    });

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !busy) {
        event.preventDefault();
        event.stopPropagation();
        onCancel();
        return;
      }
      if (event.key !== "Tab") return;

      const focusable = Array.from(
        dialogRef.current?.querySelectorAll<HTMLElement>(
          'button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), a[href], [tabindex]:not([tabindex="-1"])',
        ) ?? [],
      );
      if (focusable.length === 0) {
        event.preventDefault();
        return;
      }

      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };

    document.addEventListener("keydown", onKeyDown, true);
    return () => {
      cancelAnimationFrame(frame);
      document.removeEventListener("keydown", onKeyDown, true);
      const previous = previouslyFocusedRef.current;
      requestAnimationFrame(() => {
        if (previous?.isConnected) previous.focus();
      });
    };
  }, [busy, onCancel, open]);

  return createPortal(
    <AnimatePresence>
      {open && (
        <motion.div
          className={`${styles.overlay} glass-overlay`}
          initial={reducedMotion ? false : { opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: reducedMotion ? 0 : 0.12 }}
          onMouseDown={(event) => {
            if (!busy && event.target === event.currentTarget) onCancel();
          }}
        >
          <motion.div
            ref={dialogRef}
            className={`${styles.dialog} ${size === "compact" ? styles.compact : ""} liquid-glass`}
            role="alertdialog"
            aria-modal="true"
            aria-labelledby={titleId}
            aria-describedby={message ? messageId : undefined}
            initial={reducedMotion ? false : { opacity: 0, scale: 0.97, y: 6 }}
            animate={reducedMotion ? { opacity: 1 } : { opacity: 1, scale: 1, y: 0 }}
            exit={reducedMotion ? { opacity: 0 } : { opacity: 0, scale: 0.97, y: 6 }}
            transition={{ duration: reducedMotion ? 0 : 0.12 }}
          >
            <div className={styles.header}>
              <span className={styles.warningIcon} aria-hidden="true">
                <Icon name="warning" size="md" />
              </span>
              <div className={styles.headerText}>
                <h3 id={titleId} className={styles.title}>{title}</h3>
                {message && <div id={messageId} className={styles.message}>{message}</div>}
              </div>
            </div>

            {children && <div className={styles.body}>{children}</div>}

            <div className={styles.footer}>
              <GlassButton
                type="button"
                variant="ghost"
                size="md"
                className={styles.actionButton}
                data-action="cancel"
                disabled={busy}
                onClick={onCancel}
              >
                {t("common.cancel")}
              </GlassButton>
              <GlassButton
                type="button"
                variant={intent}
                size="md"
                className={styles.actionButton}
                data-action="confirm"
                loading={busy}
                onClick={onConfirm}
              >
                {t("common.confirm")}
              </GlassButton>
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>,
    document.body,
  );
}
