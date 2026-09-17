import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import type { SftpEntry } from "../FileManager/types";
import Icon from "../common/Icon";
import { formatBytes } from "../../utils/format";
import { REMOTE_DOCUMENT_ENCODINGS } from "./documentCodec";
import { useRemoteDocument } from "./useRemoteDocument";
import TextEditor from "./views/TextEditor";
import HexViewer from "./views/HexViewer";
import styles from "./RemoteDocumentDialog.module.css";

interface RemoteDocumentDialogProps {
  visible: boolean;
  sessionId: string;
  entry: SftpEntry;
  isConnected: boolean;
  onClose: () => void;
  onSaved: () => void;
}

function formatModified(value: number | null | undefined): string {
  if (!value) return "—";
  return new Date(value * 1000).toLocaleString();
}

export default function RemoteDocumentDialog({
  visible,
  sessionId,
  entry,
  isConnected,
  onClose,
  onSaved,
}: RemoteDocumentDialogProps) {
  const { t } = useTranslation();
  const dialogRef = useRef<HTMLDivElement>(null);
  const [confirmClose, setConfirmClose] = useState(false);
  const [cursorLine, setCursorLine] = useState(1);
  const [cursorColumn, setCursorColumn] = useState(1);
  const document = useRemoteDocument(sessionId, entry.path, isConnected, onSaved);

  const requestClose = useCallback(() => {
    if (document.dirty) {
      setConfirmClose(true);
      return;
    }
    onClose();
  }, [document.dirty, onClose]);

  const save = useCallback(() => {
    void document.save(false);
  }, [document]);

  useEffect(() => {
    if (!visible) return;
    const frame = requestAnimationFrame(() => {
      dialogRef.current
        ?.querySelector<HTMLElement>('button:not(:disabled), select:not(:disabled), textarea:not(:disabled)')
        ?.focus();
    });

    const handler = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        if (confirmClose) setConfirmClose(false);
        else requestClose();
        return;
      }
      if (event.key !== "Tab") return;
      const focusable = Array.from(
        dialogRef.current?.querySelectorAll<HTMLElement>(
          'button:not(:disabled), select:not(:disabled), input:not(:disabled), textarea:not(:disabled), [tabindex]:not([tabindex="-1"])',
        ) ?? [],
      );
      if (focusable.length === 0) return;
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

    document.addEventListener("keydown", handler, true);
    return () => {
      cancelAnimationFrame(frame);
      document.removeEventListener("keydown", handler, true);
    };
  }, [confirmClose, requestClose, visible]);

  if (!visible) return null;

  const lineCount = Math.max(1, document.text.split("\n").length);
  const saveDisabled =
    !isConnected
    || !document.canEdit
    || !document.dirty
    || document.saving
    || document.loading;
  const unicodeBom = document.format.encoding === "utf-8"
    || document.format.encoding === "utf-16le"
    || document.format.encoding === "utf-16be";

  return createPortal(
    <div className={`${styles.overlay} glass-overlay`}>
      <div
        ref={dialogRef}
        className={`${styles.container} liquid-glass`}
        role="dialog"
        aria-modal="true"
        aria-labelledby="remote-document-title"
      >
        <header className={styles.header}>
          <div className={styles.titleBlock}>
            <div className={styles.titleRow}>
              <span id="remote-document-title" className={styles.title}>{entry.name}</span>
              {document.dirty && <span className={styles.dirtyDot} aria-label="modified">●</span>}
            </div>
            <div className={styles.path} title={entry.path}>{entry.path}</div>
          </div>
          <div className={styles.headerActions}>
            <button
              type="button"
              className="liquid-glass-button liquid-primary-button"
              disabled={saveDisabled}
              onClick={save}
            >
              {document.saving ? t("fileManager.transferFinalizing") : t("common.save")}
            </button>
            <button
              type="button"
              data-action="close"
              className={`${styles.iconButton} liquid-glass-ghost-button`}
              onClick={requestClose}
              aria-label={t("common.close")}
              title={t("common.close")}
            >
              <Icon name="close" size="md" />
            </button>
          </div>
        </header>

        <div className={styles.toolbar}>
          <div className={`${styles.modeGroup} liquid-selector-strip`} role="group" aria-label={t("fileManager.previewMode")}>
            <button
              type="button"
              className={`liquid-glass-button liquid-selector-button ${document.mode === "text" ? "active" : ""}`}
              aria-pressed={document.mode === "text"}
              onClick={() => document.setMode("text")}
            >
              {t("fileManager.previewText")}
            </button>
            <button
              type="button"
              className={`liquid-glass-button liquid-selector-button ${document.mode === "hex" ? "active" : ""}`}
              aria-pressed={document.mode === "hex"}
              onClick={() => document.setMode("hex")}
            >
              HEX
            </button>
          </div>

          {document.mode === "text" && (
            <div className={styles.formatControls}>
              <label className={styles.controlLabel}>
                <span>{t("fileManager.open")} · {t("fileManager.previewEncoding")}</span>
                <select
                  className={`${styles.select} liquid-glass-input liquid-glass-select`}
                  value={document.sourceEncoding}
                  disabled={document.loading || document.saving || document.dirty}
                  onChange={(event) => void document.reopenAs(event.target.value as typeof document.sourceEncoding)}
                >
                  {REMOTE_DOCUMENT_ENCODINGS.map((encoding) => (
                    <option key={encoding} value={encoding}>{encoding.toUpperCase()}</option>
                  ))}
                </select>
              </label>

              <label className={styles.controlLabel}>
                <span>{t("common.save")} · {t("fileManager.previewEncoding")}</span>
                <select
                  className={`${styles.select} liquid-glass-input liquid-glass-select`}
                  value={document.format.encoding}
                  disabled={!document.canEdit || document.saving}
                  onChange={(event) => document.updateFormat({ encoding: event.target.value as typeof document.format.encoding })}
                >
                  {REMOTE_DOCUMENT_ENCODINGS.map((encoding) => (
                    <option key={encoding} value={encoding}>{encoding.toUpperCase()}</option>
                  ))}
                </select>
              </label>

              <label className={styles.controlLabel}>
                <span>EOL</span>
                <select
                  className={`${styles.smallSelect} liquid-glass-input liquid-glass-select`}
                  value={document.format.lineEnding}
                  disabled={!document.canEdit || document.saving}
                  onChange={(event) => document.updateFormat({ lineEnding: event.target.value as typeof document.format.lineEnding })}
                >
                  <option value="lf">LF</option>
                  <option value="crlf">CRLF</option>
                  <option value="cr">CR</option>
                </select>
              </label>

              <label className={styles.bomControl}>
                <input
                  type="checkbox"
                  checked={document.format.bom}
                  disabled={!document.canEdit || document.saving || !unicodeBom}
                  onChange={(event) => document.updateFormat({ bom: event.target.checked })}
                />
                <span>BOM</span>
              </label>
            </div>
          )}
        </div>

        {!isConnected && (
          <div className={styles.notice} role="status">
            <Icon name="status-disconnected" size="xs" />
            <span>{t("statusBar.disconnected")}</span>
          </div>
        )}

        {document.snapshot?.truncated && (
          <div className={styles.notice}>
            <span>
              {t("fileManager.previewTruncated", {
                shown: formatBytes(document.snapshot.bytes.length),
                total: formatBytes(document.snapshot.totalSize),
              })}
            </span>
            {document.canLoadFullForEdit && (
              <button
                type="button"
                className="liquid-glass-button"
                disabled={!isConnected || document.loading}
                onClick={() => void document.loadFullForEdit()}
              >
                {t("fileManager.edit")}
              </button>
            )}
          </div>
        )}

        {document.conflict && (
          <div className={`${styles.notice} ${styles.warning}`} role="alert">
            <Icon name="warning" size="sm" />
            <span>
              {t("common.warning")} · {t("fileManager.modified")}: {formatModified(document.conflict.currentVersion?.modified)}
            </span>
            <div className={styles.noticeActions}>
              <button
                type="button"
                className="liquid-glass-button"
                onClick={() => void document.reloadFromRemote()}
              >
                {t("fileManager.refresh")}
              </button>
              <button
                type="button"
                className="liquid-glass-button liquid-primary-button"
                disabled={!isConnected || document.saving}
                onClick={() => void document.save(true)}
              >
                {t("fileManager.conflictReplace")}
              </button>
            </div>
          </div>
        )}

        {document.error && (
          <div className={`${styles.notice} ${styles.error}`} role="alert">
            <Icon name="x-circle" size="sm" />
            <span className={styles.noticeText}>{document.error}</span>
            <button
              type="button"
              className={`${styles.iconButton} liquid-glass-ghost-button`}
              onClick={() => document.setError(null)}
              aria-label={t("common.close")}
            >
              <Icon name="close" size="sm" />
            </button>
          </div>
        )}

        <main className={styles.body}>
          {document.loading && (
            <div className={styles.centerState} role="status">{t("fileManager.loading")}</div>
          )}
          {!document.loading && document.snapshot && document.mode === "text" && (
            <TextEditor
              value={document.text}
              readOnly={!document.canEdit}
              onChange={document.setText}
              onSave={save}
              onCursorChange={(line, column) => {
                setCursorLine(line);
                setCursorColumn(column);
              }}
            />
          )}
          {!document.loading && document.snapshot && document.mode === "hex" && (
            <>
              {document.hexLoading ? (
                <div className={styles.centerState} role="status">{t("fileManager.loading")}</div>
              ) : (
                <HexViewer data={document.hexData} />
              )}
            </>
          )}
        </main>

        <footer className={styles.statusBar}>
          <span>{formatBytes(document.snapshot?.totalSize ?? entry.size)}</span>
          <span>
            {document.mode === "text"
              ? `${t("fileManager.lines", { count: lineCount })} · ${cursorLine}:${cursorColumn}`
              : t("fileManager.previewBytes", { count: document.hexData.length })}
          </span>
          <span>{document.format.encoding.toUpperCase()} · {document.format.lineEnding.toUpperCase()}</span>
          <span>{document.snapshot?.permissions ?? entry.permissions ?? "—"}</span>
        </footer>

        {document.mode === "hex" && document.hexTruncated && (
          <div className={styles.hexLimit}>
            {t("fileManager.previewHexLimited", { size: formatBytes(document.hexData.length) })}
          </div>
        )}

        {confirmClose && (
          <div className={styles.confirmLayer} role="alertdialog" aria-modal="true">
            <div className={`${styles.confirmCard} liquid-control-surface`}>
              <div className={styles.confirmTitle}>{t("common.warning")}</div>
              <div className={styles.confirmMessage}>{entry.name}</div>
              <div className={styles.confirmActions}>
                <button type="button" className="liquid-glass-button" onClick={() => setConfirmClose(false)}>
                  {t("common.cancel")}
                </button>
                <button type="button" className="liquid-glass-button" onClick={onClose}>
                  {t("common.close")}
                </button>
                <button
                  type="button"
                  className="liquid-glass-button liquid-primary-button"
                  disabled={saveDisabled}
                  onClick={async () => {
                    if (await document.save(false)) onClose();
                  }}
                >
                  {t("common.save")}
                </button>
              </div>
            </div>
          </div>
        )}
      </div>
    </div>,
    document.body,
  );
}
