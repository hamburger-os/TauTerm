import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import type { SftpEntry } from "../FileManager/types";
import ConfirmDialog from "../common/ConfirmDialog";
import Icon from "../common/Icon";
import { formatBytes } from "../../utils/format";
import { REMOTE_DOCUMENT_ENCODINGS } from "./documentCodec";
import { useRemoteDocument } from "./useRemoteDocument";
import TextEditor, { countTextLines } from "./views/TextEditor";
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
  const previouslyFocusedRef = useRef<HTMLElement | null>(null);
  const [confirmClose, setConfirmClose] = useState(false);
  const [cursorLine, setCursorLine] = useState(1);
  const [cursorColumn, setCursorColumn] = useState(1);
  const doc = useRemoteDocument(sessionId, entry.path, isConnected, onSaved);
  const lineCount = useMemo(() => countTextLines(doc.text), [doc.text]);

  const requestClose = useCallback(() => {
    // The SFTP document save is a transactional backend operation. Keep the editor mounted until
    // it resolves so closing can never race an already-started remote commit.
    if (doc.saving) return;
    if (doc.dirty) {
      setConfirmClose(true);
      return;
    }
    onClose();
  }, [doc.dirty, doc.saving, onClose]);

  // The keyboard listener is document-scoped, so keep its dynamic decisions in refs instead
  // of tearing the listener down on every dirty/saving state change while the user is typing.
  const requestCloseRef = useRef(requestClose);
  requestCloseRef.current = requestClose;
  const confirmCloseStateRef = useRef(confirmClose);
  confirmCloseStateRef.current = confirmClose;
  const savingStateRef = useRef(doc.saving);
  savingStateRef.current = doc.saving;

  const save = useCallback(() => {
    void doc.save(false);
  }, [doc]);

  useEffect(() => {
    if (!visible) return;
    previouslyFocusedRef.current = document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null;
    const frame = requestAnimationFrame(() => dialogRef.current?.focus());
    return () => {
      cancelAnimationFrame(frame);
      const previous = previouslyFocusedRef.current;
      previouslyFocusedRef.current = null;
      requestAnimationFrame(() => {
        if (previous?.isConnected) previous.focus();
      });
    };
  }, [visible]);

  useEffect(() => {
    if (!visible) return;
    const handler = (event: KeyboardEvent) => {
      // Let focused child widgets own keys they deliberately consume. In particular,
      // TextEditor uses Tab for indentation and Ctrl/Cmd+S for save without losing caret focus.
      if (event.defaultPrevented) return;
      if (confirmCloseStateRef.current) return;

      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        if (savingStateRef.current) return;
        requestCloseRef.current();
        return;
      }
      if (event.key !== "Tab") return;

      const focusRoot = dialogRef.current;
      const focusable = Array.from(
        focusRoot?.querySelectorAll<HTMLElement>(
          'button:not(:disabled), select:not(:disabled), input:not(:disabled), textarea:not(:disabled), a[href], [tabindex]:not([tabindex="-1"])',
        ) ?? [],
      );
      if (focusable.length === 0) {
        event.preventDefault();
        focusRoot?.focus();
        return;
      }

      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      const active = document.activeElement;
      if (!focusRoot?.contains(active)) {
        event.preventDefault();
        (event.shiftKey ? last : first).focus();
      } else if (event.shiftKey && active === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && active === last) {
        event.preventDefault();
        first.focus();
      }
    };

    // Bubble-phase handling lets component-level keyboard semantics run first.
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, [visible]);

  if (!visible) return null;

  const saveDisabled =
    !isConnected
    || !doc.canEdit
    || !doc.dirty
    || doc.saving
    || doc.loading;
  const unicodeBom = doc.format.encoding === "utf-8"
    || doc.format.encoding === "utf-16le"
    || doc.format.encoding === "utf-16be";
  const bomDisabled = !doc.canEdit || doc.saving || !unicodeBom;

  return createPortal(
    <div className={`${styles.overlay} glass-overlay`}>
      <div
        ref={dialogRef}
        className={`${styles.container} liquid-glass`}
        role="dialog"
        aria-modal="true"
        aria-busy={doc.saving || undefined}
        aria-labelledby="remote-document-title"
        tabIndex={-1}
      >
        <header className={styles.header}>
          <div className={styles.titleBlock}>
            <div className={styles.titleRow}>
              <span id="remote-document-title" className={styles.title}>{entry.name}</span>
              {doc.dirty && <span className={styles.dirtyDot} aria-label={t("fileManager.unsavedChanges")}>●</span>}
            </div>
            <div className={styles.path} title={entry.path}>{entry.path}</div>
          </div>
          <button
            type="button"
            data-action="close"
            className={`${styles.iconButton} liquid-glass-ghost-button`}
            disabled={doc.saving}
            onClick={requestClose}
            aria-label={t("common.close")}
            title={t("common.close")}
          >
            <Icon name="close" size="md" />
          </button>
        </header>

        <div className={styles.toolbar}>
          <div className={`${styles.modeGroup} liquid-selector-strip`} role="group" aria-label={t("fileManager.previewMode")}>
            <button
              type="button"
              className={`liquid-glass-button liquid-selector-button ${doc.mode === "text" ? "active" : ""}`}
              aria-pressed={doc.mode === "text"}
              onClick={() => doc.setMode("text")}
            >
              {t("fileManager.previewText")}
            </button>
            <button
              type="button"
              className={`liquid-glass-button liquid-selector-button ${doc.mode === "hex" ? "active" : ""}`}
              aria-pressed={doc.mode === "hex"}
              onClick={() => doc.setMode("hex")}
            >
              HEX
            </button>
          </div>

          <div className={styles.toolbarActions}>
            {doc.mode === "text" && (
              <div className={styles.formatControls}>
                <label className={styles.controlLabel}>
                  <span>{t("fileManager.open")} · {t("fileManager.previewEncoding")}</span>
                  <select
                    className={`${styles.select} liquid-glass-input liquid-glass-select`}
                    value={doc.sourceEncoding}
                    disabled={doc.loading || doc.saving || doc.dirty}
                    onChange={(event) => void doc.reopenAs(event.target.value as typeof doc.sourceEncoding)}
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
                    value={doc.format.encoding}
                    disabled={!doc.canEdit || doc.saving}
                    onChange={(event) => doc.updateFormat({ encoding: event.target.value as typeof doc.format.encoding })}
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
                    value={doc.format.lineEnding}
                    disabled={!doc.canEdit || doc.saving}
                    onChange={(event) => doc.updateFormat({ lineEnding: event.target.value as typeof doc.format.lineEnding })}
                  >
                    <option value="lf">LF</option>
                    <option value="crlf">CRLF</option>
                    <option value="cr">CR</option>
                  </select>
                </label>

                <div className={styles.controlLabel}>
                  <span>BOM</span>
                  <label
                    className={`${styles.bomToggle} liquid-glass-toggle ${bomDisabled ? styles.bomToggleDisabled : ""}`.trim()}
                    title="BOM"
                  >
                    <input
                      type="checkbox"
                      aria-label="BOM"
                      checked={doc.format.bom}
                      disabled={bomDisabled}
                      onChange={(event) => doc.updateFormat({ bom: event.target.checked })}
                    />
                    <div aria-hidden="true" />
                  </label>
                </div>
              </div>
            )}

            <button
              type="button"
              className={`${styles.saveButton} liquid-glass-button liquid-primary-button`}
              disabled={saveDisabled}
              onClick={save}
              aria-keyshortcuts="Control+S Meta+S"
            >
              {doc.saving ? t("fileManager.transferFinalizing") : t("common.save")}
            </button>
          </div>
        </div>

        {!isConnected && (
          <div className={styles.notice} role="status">
            <Icon name="status-disconnected" size="xs" />
            <span>{t("statusBar.disconnected")}</span>
          </div>
        )}

        {doc.snapshot?.truncated && (
          <div className={styles.notice}>
            <span>
              {t("fileManager.previewTruncated", {
                shown: formatBytes(doc.snapshot.bytes.length),
                total: formatBytes(doc.snapshot.totalSize),
              })}
            </span>
            {doc.canLoadFullForEdit && (
              <button
                type="button"
                className="liquid-glass-button"
                disabled={!isConnected || doc.loading}
                onClick={() => void doc.loadFullForEdit()}
              >
                {t("fileManager.edit")}
              </button>
            )}
          </div>
        )}

        {doc.conflict && (
          <div className={`${styles.notice} ${styles.warning}`} role="alert">
            <Icon name="warning" size="sm" />
            <span>
              {t("common.warning")} · {t("fileManager.modified")}: {formatModified(doc.conflict.currentVersion?.modified)}
            </span>
            <div className={styles.noticeActions}>
              <button
                type="button"
                className="liquid-glass-button"
                onClick={() => void doc.reloadFromRemote()}
              >
                {t("fileManager.refresh")}
              </button>
              <button
                type="button"
                className="liquid-glass-button liquid-primary-button"
                disabled={!isConnected || doc.saving}
                onClick={() => void doc.save(true)}
              >
                {t("fileManager.conflictReplace")}
              </button>
            </div>
          </div>
        )}

        {doc.error && (
          <div className={`${styles.notice} ${styles.error}`} role="alert">
            <Icon name="x-circle" size="sm" />
            <span className={styles.noticeText}>{doc.error}</span>
            <button
              type="button"
              className={`${styles.iconButton} liquid-glass-ghost-button`}
              onClick={() => doc.setError(null)}
              aria-label={t("common.close")}
            >
              <Icon name="close" size="sm" />
            </button>
          </div>
        )}

        <main className={styles.body}>
          {doc.loading && (
            <div className={styles.centerState} role="status">{t("fileManager.loading")}</div>
          )}
          {!doc.loading && doc.snapshot && doc.mode === "text" && (
            <TextEditor
              value={doc.text}
              readOnly={!doc.canEdit}
              autoFocus
              onChange={doc.setText}
              onSave={save}
              onCursorChange={(line, column) => {
                setCursorLine(line);
                setCursorColumn(column);
              }}
            />
          )}
          {!doc.loading && doc.snapshot && doc.mode === "hex" && (
            <>
              {doc.hexLoading ? (
                <div className={styles.centerState} role="status">{t("fileManager.loading")}</div>
              ) : (
                <HexViewer data={doc.hexData} />
              )}
            </>
          )}
        </main>

        <footer className={styles.statusBar}>
          <span>{formatBytes(doc.snapshot?.totalSize ?? entry.size)}</span>
          <span>
            {doc.mode === "text"
              ? `${t("fileManager.lines", { count: lineCount })} · ${cursorLine}:${cursorColumn}`
              : t("fileManager.previewBytes", { count: doc.hexData.length })}
          </span>
          <span>{doc.format.encoding.toUpperCase()} · {doc.format.lineEnding.toUpperCase()}</span>
          <span>{doc.snapshot?.permissions ?? entry.permissions ?? "—"}</span>
        </footer>

        {doc.mode === "hex" && doc.hexTruncated && (
          <div className={styles.hexLimit}>
            {t("fileManager.previewHexLimited", { size: formatBytes(doc.hexData.length) })}
          </div>
        )}

        <ConfirmDialog
          open={confirmClose}
          title={t("fileManager.saveChangesConfirm")}
          message={entry.name}
          busy={doc.saving}
          onCancel={() => setConfirmClose(false)}
          onConfirm={() => {
            void doc.save(false).then((saved) => {
              if (saved) {
                onClose();
              } else {
                setConfirmClose(false);
              }
            });
          }}
        />
      </div>
    </div>,
    document.body,
  );
}
