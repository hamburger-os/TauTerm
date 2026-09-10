import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { analyzeTerminalPaste, buildTerminalPastePreview } from "../../utils/terminalClipboard";
import ConfirmDialog from "../common/ConfirmDialog";
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

  const analysis = useMemo(
    () => analyzeTerminalPaste(text ?? ""),
    [text],
  );
  const preview = useMemo(
    () => buildTerminalPastePreview(text ?? ""),
    [text],
  );

  return (
    <ConfirmDialog
      open={text !== null}
      title={t("terminal.pasteWarningTitle")}
      message={t("terminal.pasteWarningMessage", {
        lines: analysis.contentLineCount,
      })}
      onConfirm={onConfirm}
      onCancel={onCancel}
    >
      <div className={styles.content}>
        <div className={styles.meta}>
          {t("terminal.pasteWarningMeta", {
            lines: analysis.contentLineCount,
            characters: analysis.characterCount,
          })}
        </div>

        <div className={styles.previewLabel}>{t("terminal.pasteWarningPreview")}</div>
        <pre className={`${styles.preview} liquid-control-surface`}>
          {preview.preview}
          {preview.truncated ? "\n…" : ""}
        </pre>
      </div>
    </ConfirmDialog>
  );
}
