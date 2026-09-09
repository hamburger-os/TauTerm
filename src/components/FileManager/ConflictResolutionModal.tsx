import { useEffect, useRef } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import type { OverwritePolicy } from "./types";
import styles from "./ConflictResolutionModal.module.css";

interface ConflictResolutionModalProps {
  visible: boolean;
  conflictCount: number;
  onResolve: (policy: OverwritePolicy | null) => void;
}

export default function ConflictResolutionModal({
  visible,
  conflictCount,
  onResolve,
}: ConflictResolutionModalProps) {
  const { t } = useTranslation();
  const dialogRef = useRef<HTMLDivElement>(null);
  const keepBothRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!visible) return;
    const frame = requestAnimationFrame(() => keepBothRef.current?.focus());
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        onResolve(null);
        return;
      }
      if (event.key === "Tab") {
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
      }
    };
    document.addEventListener("keydown", onKeyDown, true);
    return () => {
      cancelAnimationFrame(frame);
      document.removeEventListener("keydown", onKeyDown, true);
    };
  }, [visible, onResolve]);

  if (!visible) return null;

  return createPortal(
    <div
      className={`${styles.overlay} glass-overlay`}
      role="presentation"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onResolve(null);
      }}
    >
      <div
        ref={dialogRef}
        className={`${styles.container} liquid-glass`}
        role="dialog"
        aria-modal="true"
        aria-labelledby="file-conflict-title"
      >
        <h3 id="file-conflict-title" className={styles.title}>
          {t("fileManager.conflictTitle")}
        </h3>
        <p className={styles.message}>
          {t("fileManager.conflictMessage", { count: conflictCount })}
        </p>
        <div className={styles.actions}>
          <button
            className={`${styles.action} liquid-glass-button`}
            onClick={() => onResolve("replace")}
          >
            {t("fileManager.conflictReplace")}
          </button>
          <button
            ref={keepBothRef}
            className={`${styles.action} liquid-glass-button`}
            onClick={() => onResolve("keep-both")}
          >
            {t("fileManager.conflictKeepBoth")}
          </button>
          <button
            className={`${styles.action} liquid-glass-button`}
            onClick={() => onResolve("skip")}
          >
            {t("fileManager.conflictSkip")}
          </button>
          <button
            className={`${styles.action} liquid-glass-ghost-button`}
            onClick={() => onResolve(null)}
          >
            {t("common.cancel")}
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}
