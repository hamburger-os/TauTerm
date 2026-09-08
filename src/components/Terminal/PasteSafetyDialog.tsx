import { useEffect, useMemo, useRef } from "react";
import { createPortal } from "react-dom";
import { AnimatePresence, motion } from "framer-motion";
import { useTranslation } from "react-i18next";
import { analyzeTerminalPaste, buildTerminalPastePreview } from "../../utils/terminalClipboard";
import Icon from "../common/Icon";
import styles from "./PasteSafetyDialog.module.css";

interface PasteSafetyDialogProps {
  text: string | null;
  onConfirm: () => void;
  onCancel: () => void;
}

export default function PasteSafetyDialog({
  text,
  onConfirm,
  onCancel,
}: PasteSafetyDialogProps) {
  const { t } = useTranslation();
  const cancelButtonRef = useRef<HTMLButtonElement>(null);
  const isOpen = text !== null;

  const analysis = useMemo(
    () => analyzeTerminalPaste(text ?? ""),
    [text],
  );
  const preview = useMemo(
    () => buildTerminalPastePreview(text ?? ""),
    [text],
  );

  useEffect(() => {
    if (!isOpen) return;
    const frame = requestAnimationFrame(() => cancelButtonRef.current?.focus());
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        onCancel();
      }
    };
    document.addEventListener("keydown", handleKeyDown, true);
    return () => {
      cancelAnimationFrame(frame);
      document.removeEventListener("keydown", handleKeyDown, true);
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
            className={`${styles.dialog} liquid-glass-float`}
            role="alertdialog"
            aria-modal="true"
            aria-labelledby="terminal-paste-warning-title"
            aria-describedby="terminal-paste-warning-message"
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
                <h3 id="terminal-paste-warning-title" className={styles.title}>
                  {t("terminal.pasteWarningTitle")}
                </h3>
                <p id="terminal-paste-warning-message" className={styles.message}>
                  {t("terminal.pasteWarningMessage", {
                    lines: analysis.contentLineCount,
                  })}
                </p>
              </div>
            </div>

            <div className={styles.meta}>
              {t("terminal.pasteWarningMeta", {
                lines: analysis.contentLineCount,
                characters: analysis.characterCount,
              })}
            </div>

            <div className={styles.previewLabel}>{t("terminal.pasteWarningPreview")}</div>
            <pre className={styles.preview}>
              {preview.preview}
              {preview.truncated ? "\n…" : ""}
            </pre>

            <div className={styles.actions}>
              <button
                ref={cancelButtonRef}
                type="button"
                className={`${styles.button} liquid-glass-ghost-button`}
                onClick={onCancel}
              >
                {t("terminal.pasteWarningCancel")}
              </button>
              <button
                type="button"
                className={`${styles.button} liquid-glass-button`}
                onClick={onConfirm}
              >
                {t("terminal.pasteWarningConfirm")}
              </button>
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>,
    document.body,
  );
}
