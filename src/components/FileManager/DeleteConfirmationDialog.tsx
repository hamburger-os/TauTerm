import { useEffect, useRef } from "react";
import { createPortal } from "react-dom";
import { AnimatePresence, motion } from "framer-motion";
import { useTranslation } from "react-i18next";
import Icon from "../common/Icon";
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
  const cancelRef = useRef<HTMLButtonElement>(null);
  const deleteRef = useRef<HTMLButtonElement>(null);
  const isOpen = message !== null;

  useEffect(() => {
    if (!isOpen) return;
    const frame = requestAnimationFrame(() => cancelRef.current?.focus());
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        onCancel();
        return;
      }
      if (event.key === "Tab") {
        const active = document.activeElement;
        if (event.shiftKey && active === cancelRef.current) {
          event.preventDefault();
          deleteRef.current?.focus();
        } else if (!event.shiftKey && active === deleteRef.current) {
          event.preventDefault();
          cancelRef.current?.focus();
        }
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
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.12 }}
          onMouseDown={(event) => {
            if (event.target === event.currentTarget) onCancel();
          }}
        >
          <motion.div
            className={`${styles.dialog} liquid-glass`}
            role="alertdialog"
            aria-modal="true"
            aria-labelledby="file-delete-title"
            aria-describedby="file-delete-message"
            initial={{ opacity: 0, scale: 0.97, y: 6 }}
            animate={{ opacity: 1, scale: 1, y: 0 }}
            exit={{ opacity: 0, scale: 0.97, y: 6 }}
            transition={{ duration: 0.12 }}
          >
            <div className={styles.header}>
              <span className={styles.warningIcon} aria-hidden="true">
                <Icon name="warning" size="md" />
              </span>
              <div>
                <h3 id="file-delete-title" className={styles.title}>
                  {t("fileManager.deleteConfirmTitle")}
                </h3>
                <p id="file-delete-message" className={styles.message}>
                  {message}
                </p>
              </div>
            </div>
            <div className={styles.actions}>
              <button
                ref={cancelRef}
                type="button"
                className={`${styles.button} liquid-glass-ghost-button`}
                onClick={onCancel}
              >
                {t("common.cancel")}
              </button>
              <button
                ref={deleteRef}
                type="button"
                className={`${styles.button} ${styles.dangerButton} liquid-glass-button`}
                onClick={onConfirm}
              >
                {t("fileManager.deleteConfirmAction")}
              </button>
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>,
    document.body,
  );
}
