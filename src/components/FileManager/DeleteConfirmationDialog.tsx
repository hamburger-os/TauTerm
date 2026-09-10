import { useEffect, useRef } from "react";
import { createPortal } from "react-dom";
import { AnimatePresence, motion, useReducedMotion } from "framer-motion";
import { useTranslation } from "react-i18next";
import Icon from "../common/Icon";
import GlassButton from "../common/GlassButton";
import styles from "./DeleteConfirmationDialog.module.css";

interface DeleteConfirmationDialogProps {
  message: string | null;
  onConfirm: () => void;
  onCancel: () => void;
}

export default function DeleteConfirmationDialog({
  message,
  onConfirm,
  onCancel,
}: DeleteConfirmationDialogProps) {
  const { t } = useTranslation();
  const dialogRef = useRef<HTMLDivElement>(null);
  const reducedMotion = useReducedMotion();
  const isOpen = message !== null;

  useEffect(() => {
    if (!isOpen) return;
    const frame = requestAnimationFrame(() => {
      dialogRef.current
        ?.querySelector<HTMLButtonElement>('[data-action="cancel"]')
        ?.focus();
    });

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        onCancel();
        return;
      }
      if (event.key !== "Tab") return;

      const buttons = Array.from(
        dialogRef.current?.querySelectorAll<HTMLButtonElement>("button:not(:disabled)") ?? [],
      );
      if (buttons.length === 0) return;
      const first = buttons[0];
      const last = buttons[buttons.length - 1];
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
    };
  }, [isOpen, onCancel]);

  return createPortal(
    <AnimatePresence>
      {isOpen && (
        <motion.div
          className={`${styles.overlay} glass-overlay`}
          initial={reducedMotion ? false : { opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: reducedMotion ? 0 : 0.12 }}
          onMouseDown={(event) => {
            if (event.target === event.currentTarget) onCancel();
          }}
        >
          <motion.div
            ref={dialogRef}
            className={`${styles.dialog} liquid-glass`}
            role="alertdialog"
            aria-modal="true"
            aria-labelledby="file-delete-title"
            aria-describedby="file-delete-message"
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
                <h3 id="file-delete-title" className={styles.title}>
                  {t("fileManager.deleteConfirmTitle")}
                </h3>
                <p id="file-delete-message" className={styles.message}>
                  {message}
                </p>
              </div>
            </div>

            <div className={styles.footer}>
              <GlassButton
                type="button"
                variant="ghost"
                size="md"
                className={styles.actionButton}
                data-action="cancel"
                onClick={onCancel}
              >
                {t("common.cancel")}
              </GlassButton>
              <GlassButton
                type="button"
                variant="danger"
                size="md"
                className={styles.actionButton}
                data-action="delete"
                onClick={onConfirm}
              >
                {t("fileManager.deleteConfirmAction")}
              </GlassButton>
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>,
    document.body,
  );
}
