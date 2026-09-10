import { useEffect, useRef } from "react";
import { createPortal } from "react-dom";
import { AnimatePresence, motion } from "framer-motion";
import { useTranslation } from "react-i18next";
import Icon from "../common/Icon";
import GlassButton from "../common/GlassButton";
import type { OverwritePolicy } from "./types";
import styles from "./ConflictResolutionModal.module.css";

interface ConflictResolutionModalProps {
  visible: boolean;
  conflictCount: number;
  allowReplace?: boolean;
  onResolve: (policy: OverwritePolicy | null) => void;
}

export default function ConflictResolutionModal({
  visible,
  conflictCount,
  allowReplace = true,
  onResolve,
}: ConflictResolutionModalProps) {
  const { t } = useTranslation();
  const dialogRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!visible) return;
    const frame = requestAnimationFrame(() => {
      dialogRef.current
        ?.querySelector<HTMLButtonElement>('[data-policy="keep-both"]')
        ?.focus();
    });

    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        onResolve(null);
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
  }, [visible, onResolve]);

  return createPortal(
    <AnimatePresence>
      {visible && (
        <motion.div
          className={`${styles.overlay} glass-overlay`}
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.12 }}
          onMouseDown={(event) => {
            if (event.target === event.currentTarget) onResolve(null);
          }}
        >
          <motion.div
            ref={dialogRef}
            className={`${styles.dialog} liquid-glass`}
            role="alertdialog"
            aria-modal="true"
            aria-labelledby="file-conflict-title"
            aria-describedby="file-conflict-message"
            initial={{ opacity: 0, scale: 0.97, y: 6 }}
            animate={{ opacity: 1, scale: 1, y: 0 }}
            exit={{ opacity: 0, scale: 0.97, y: 6 }}
            transition={{ duration: 0.12 }}
          >
            <div className={styles.header}>
              <span className={styles.warningIcon} aria-hidden="true">
                <Icon name="warning" size="md" />
              </span>
              <div className={styles.headerText}>
                <h3 id="file-conflict-title" className={styles.title}>
                  {t("fileManager.conflictTitle")}
                </h3>
                <p id="file-conflict-message" className={styles.message}>
                  {t("fileManager.conflictMessage", { count: conflictCount })}
                </p>
              </div>
            </div>

            <div
              className={styles.policyList}
              role="group"
              aria-label={t("fileManager.conflictChoicesLabel")}
            >
              {allowReplace && (
                <div className={styles.policyRow}>
                  <GlassButton
                    type="button"
                    variant="danger"
                    fullWidth
                    data-policy="replace"
                    onClick={() => onResolve("replace")}
                  >
                    {t("fileManager.conflictReplace")}
                  </GlassButton>
                  <span className={styles.policyHint}>
                    {t("fileManager.conflictReplaceHint")}
                  </span>
                </div>
              )}

              <div className={styles.policyRow}>
                <GlassButton
                  type="button"
                  variant="secondary"
                  fullWidth
                  data-policy="keep-both"
                  onClick={() => onResolve("keep-both")}
                >
                  {t("fileManager.conflictKeepBoth")}
                </GlassButton>
                <span className={styles.policyHint}>
                  {t("fileManager.conflictKeepBothHint")}
                </span>
              </div>

              <div className={styles.policyRow}>
                <GlassButton
                  type="button"
                  variant="secondary"
                  fullWidth
                  data-policy="skip"
                  onClick={() => onResolve("skip")}
                >
                  {t("fileManager.conflictSkip")}
                </GlassButton>
                <span className={styles.policyHint}>
                  {t("fileManager.conflictSkipHint")}
                </span>
              </div>
            </div>

            <div className={styles.footer}>
              <GlassButton
                type="button"
                variant="ghost"
                size="md"
                onClick={() => onResolve(null)}
              >
                {t("common.cancel")}
              </GlassButton>
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>,
    document.body,
  );
}
