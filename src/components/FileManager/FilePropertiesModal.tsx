/**
 * 文件属性弹窗
 *
 * 显示远程文件/目录的详细元数据。
 * 主题样式参照 SettingsPage (glass-overlay + liquid-glass)。
 */
import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import Icon from "../common/Icon";
import GlassButton from "../common/GlassButton";
import { invoke } from "@tauri-apps/api/core";
import { formatBytes, formatTime } from "../../utils/format";
import type { SftpEntry, SftpEntryType } from "./types";
import { copyToClipboard } from "../../utils/clipboard";
import { sftpErrorMessage } from "../../services/sftpErrors";
import { getEntryIcon } from "./entryIcon";
import { isDirectory, isRegularFile, isSymlink } from "./entryKind";
import type { ResolvedEntry } from "./entryActivation";
import { resolutionStatusKey } from "./entryActivation";
import styles from "./FilePropertiesModal.module.css";

/** 类型标签的 i18n key；仅用于属性展示，不参与任何类型判定。 */
function entryTypeLabelKey(entryType: SftpEntryType): string {
  switch (entryType) {
    case "directory":
      return "fileManager.typeDir";
    case "file":
      return "fileManager.typeFile";
    case "symlink":
      return "fileManager.typeSymlink";
    default:
      return "fileManager.typeOther";
  }
}

interface FilePropertiesModalProps {
  visible: boolean;
  entry: SftpEntry | null;
  statInfo: SftpEntry | null;
  /** 符号链接的解析结果；非链接或尚未解析时为 null。 */
  resolution: ResolvedEntry | null;
  loading: boolean;
  onClose: () => void;
  sessionId: string;
  onChmodComplete?: () => void;
}

// ── Component ──────────────────────────────────────────

export default function FilePropertiesModal({
  visible,
  entry,
  statInfo,
  resolution,
  loading,
  onClose,
  sessionId,
  onChmodComplete,
}: FilePropertiesModalProps) {
  const { t } = useTranslation();
  const dialogRef = useRef<HTMLDivElement>(null);
  const activeRef = useRef(false);

  useEffect(() => {
    activeRef.current = visible;
    return () => {
      activeRef.current = false;
    };
  }, [visible]);

  // ── Chmod state ──
  const [chmodValue, setChmodValue] = useState("");
  const [chmodEditing, setChmodEditing] = useState(false);
  const [chmodError, setChmodError] = useState<string | null>(null);
  const chmodEditingRef = useRef(chmodEditing);
  chmodEditingRef.current = chmodEditing;
  const cancelChmodEditRef = useRef<() => void>(() => {});

  // Extract chmod octal from a POSIX permission string. Preserve setuid,
  // setgid and sticky bits so editing rwx permissions never clears an existing
  // special bit accidentally (e.g. "-rwsr-xr-x" -> "4755").
  const getOctalFromPerms = useCallback((perms: string | null): string => {
    if (!perms || perms.length < 10) return "";

    let mode = 0;
    if (perms[1] === "r") mode += 0o400;
    if (perms[2] === "w") mode += 0o200;
    if (perms[3] === "x" || perms[3] === "s") mode += 0o100;
    if (perms[4] === "r") mode += 0o040;
    if (perms[5] === "w") mode += 0o020;
    if (perms[6] === "x" || perms[6] === "s") mode += 0o010;
    if (perms[7] === "r") mode += 0o004;
    if (perms[8] === "w") mode += 0o002;
    if (perms[9] === "x" || perms[9] === "t") mode += 0o001;

    if (perms[3] === "s" || perms[3] === "S") mode += 0o4000;
    if (perms[6] === "s" || perms[6] === "S") mode += 0o2000;
    if (perms[9] === "t" || perms[9] === "T") mode += 0o1000;

    return mode > 0o777
      ? mode.toString(8).padStart(4, "0")
      : mode.toString(8).padStart(3, "0");
  }, []);

  // Reset chmod whenever the inspected target changes, including targets whose
  // server does not report POSIX permission bits. This prevents stale values from
  // a previously opened file from leaking into the next Properties dialog.
  useEffect(() => {
    setChmodValue(getOctalFromPerms(statInfo?.permissions ?? null));
    setChmodEditing(false);
    setChmodError(null);
  }, [statInfo?.path, statInfo?.permissions, getOctalFromPerms]);

  const cancelChmodEdit = useCallback(() => {
    setChmodEditing(false);
    setChmodError(null);
    setChmodValue(getOctalFromPerms(statInfo?.permissions ?? null));
  }, [statInfo?.permissions, getOctalFromPerms]);

  cancelChmodEditRef.current = cancelChmodEdit;

  const handleChmodApply = useCallback(async () => {
    if (!/^[0-7]{3,4}$/.test(chmodValue)) {
      setChmodError(t("fileManager.chmodInvalid"));
      return;
    }
    const mode = parseInt(chmodValue, 8);
    try {
      await invoke<void>("sftp_chmod_cmd", {
        sessionId,
        remotePath: statInfo!.path,
        mode,
      });
      if (!activeRef.current) return;
      setChmodEditing(false);
      setChmodError(null);
      onChmodComplete?.();
    } catch (error) {
      if (!activeRef.current) return;
      setChmodError(sftpErrorMessage(error));
    }
  }, [chmodValue, sessionId, statInfo, t, onChmodComplete]);

  const handleOverlayClick = useCallback(
    (e: React.MouseEvent) => {
      if (e.target === e.currentTarget) onClose();
    },
    [onClose]
  );

  const handleCopyPath = useCallback(async () => {
    if (statInfo?.path) {
      try {
        await copyToClipboard(statInfo.path);
      } catch {
        // Clipboard API may not be available
      }
    }
  }, [statInfo]);

  const copyLinkValue = useCallback(async (value: string | null | undefined) => {
    if (!value) return;
    try {
      await copyToClipboard(value);
    } catch {
      // Clipboard API may not be available
    }
  }, []);

  // ── Modal keyboard boundary ──
  useEffect(() => {
    if (!visible) return;

    const frame = requestAnimationFrame(() => {
      dialogRef.current
        ?.querySelector<HTMLButtonElement>('[data-action="close"]')
        ?.focus();
    });

    const handler = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        if (chmodEditingRef.current) {
          cancelChmodEditRef.current();
        } else {
          onClose();
        }
        return;
      }
      if (event.key !== "Tab") return;

      const focusable = Array.from(
        dialogRef.current?.querySelectorAll<HTMLElement>(
          'button:not(:disabled), input:not(:disabled), [tabindex]:not([tabindex="-1"])',
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
  }, [visible, onClose]);

  if (!visible || !entry) return null;

  // 类型唯一真源仍是 entry_type：stat 与列表项一致，stat 结果优先。
  const entryType = statInfo?.entry_type ?? entry.entry_type;
  const kindSource = { entry_type: entryType };
  const isDir = isDirectory(kindSource);
  const isLink = isSymlink(kindSource);
  const isFile = isRegularFile(kindSource);
  const typeLabel = t(entryTypeLabelKey(entryType));
  const entryIcon = getEntryIcon({ ...entry, entry_type: entryType });
  // SFTP v3 的 chmod 无法可靠地 no-follow，仅对普通文件/目录开放。
  const canChmod = isFile || isDir;
  const sizeUnavailable = isDir || isLink;
  const linkStatusKey = resolution ? resolutionStatusKey(resolution) : null;
  const linkTargetType = resolution?.target_type ?? null;
  const linkStatusText = resolution === null
    ? "—"
    : linkStatusKey !== null
      ? t(linkStatusKey)
      : linkTargetType !== null && isDirectory({ entry_type: linkTargetType })
        ? t("fileManager.typeDir")
        : linkTargetType !== null && isRegularFile({ entry_type: linkTargetType })
          ? t("fileManager.typeFile")
          : t("fileManager.typeOther");
  const name = statInfo?.name ?? entry.name;

  return createPortal(
    <div
      className={`${styles.overlay} glass-overlay`}
      onClick={handleOverlayClick}
    >
      <div
        ref={dialogRef}
        className={`${styles.container} liquid-glass`}
        role="dialog"
        aria-modal="true"
        aria-labelledby="file-properties-title"
      >
        {/* 标题栏 */}
        <div className={styles.header}>
          <span id="file-properties-title" className={styles.headerTitle}>
            <span className={styles.headerIcon} aria-hidden="true">{entryIcon}</span>
            <span className={styles.headerName}>{name}</span>
          </span>
          <button
            type="button"
            data-action="close"
            className={`${styles.closeBtn} liquid-glass-ghost-button`}
            onClick={onClose}
            aria-label={t("common.close")}
          >
            <Icon name="close" size="md" />
          </button>
        </div>

        {/* 内容区 */}
        <div className={styles.body}>
          {loading ? (
            <div className={styles.loading}>{t("fileManager.loading")}</div>
          ) : statInfo ? (
            <>
              <div className={styles.fieldList}>
                {/* 完整路径 */}
                <div className={styles.fieldRow}>
                  <span className={styles.fieldLabel}>{t("fileManager.path")}</span>
                  <div className={styles.fieldValueRow}>
                    <code className={styles.fieldValuePath}>{statInfo.path}</code>
                    <button
                      type="button"
                      className={`${styles.copyBtn} liquid-glass-ghost-button`}
                      onClick={handleCopyPath}
                      title={t("fileManager.copyPath")}
                      aria-label={t("fileManager.copyPath")}
                    >
                      <Icon name="clipboard" size="sm" />
                    </button>
                  </div>
                </div>

                {/* 符号链接：目标原文、真实路径与目标状态；两段路径都可单独复制 */}
                {isLink && (
                  <>
                    <div className={styles.fieldRow}>
                      <span className={styles.fieldLabel}>{t("fileManager.linkTarget")}</span>
                      <div className={styles.fieldValueRow}>
                        <code className={styles.fieldValueMono}>
                          {resolution?.link_target ?? "—"}
                        </code>
                        <button
                          type="button"
                          className={`${styles.copyBtn} liquid-glass-ghost-button`}
                          onClick={() => void copyLinkValue(resolution?.link_target)}
                          disabled={!resolution?.link_target}
                          title={t("fileManager.copyLinkTarget")}
                          aria-label={t("fileManager.copyLinkTarget")}
                        >
                          <Icon name="clipboard" size="sm" />
                        </button>
                      </div>
                    </div>
                    <div className={styles.fieldRow}>
                      <span className={styles.fieldLabel}>{t("fileManager.linkTargetPath")}</span>
                      <div className={styles.fieldValueRow}>
                        <code className={styles.fieldValueMono}>
                          {resolution?.target_path ?? "—"}
                        </code>
                        <button
                          type="button"
                          className={`${styles.copyBtn} liquid-glass-ghost-button`}
                          onClick={() => void copyLinkValue(resolution?.target_path)}
                          disabled={!resolution?.target_path}
                          title={t("fileManager.copyPath")}
                          aria-label={t("fileManager.copyPath")}
                        >
                          <Icon name="clipboard" size="sm" />
                        </button>
                      </div>
                    </div>
                    <div className={styles.fieldRow}>
                      <span className={styles.fieldLabel}>{t("fileManager.linkTargetStatus")}</span>
                      <span
                        className={styles.fieldValue}
                        title={resolution?.resolution_error ?? undefined}
                      >
                        {linkStatusText}
                      </span>
                    </div>
                  </>
                )}

                {/* 类型 */}
                <div className={styles.fieldRow}>
                  <span className={styles.fieldLabel}>{t("fileManager.type")}</span>
                  <span className={styles.fieldValue}>{typeLabel}</span>
                </div>

                {/* 大小：目录与符号链接不展示条目大小 */}
                <div className={styles.fieldRow}>
                  <span className={styles.fieldLabel}>{t("fileManager.size")}</span>
                  <span className={styles.fieldValue}>
                    {sizeUnavailable ? "-" : formatBytes(statInfo.size)}
                  </span>
                </div>

                {/* 修改时间 */}
                <div className={styles.fieldRow}>
                  <span className={styles.fieldLabel}>{t("fileManager.modified")}</span>
                  <span className={styles.fieldValue}>{formatTime(statInfo.modified)}</span>
                </div>

                {/* 访问时间 */}
                <div className={styles.fieldRow}>
                  <span className={styles.fieldLabel}>{t("fileManager.accessed")}</span>
                  <span className={styles.fieldValue}>{formatTime(statInfo.accessed)}</span>
                </div>

                {/* 权限 */}
                <div className={styles.fieldRow}>
                  <span className={styles.fieldLabel}>{t("fileManager.permissions")}</span>
                  <code className={styles.fieldValueMono}>{statInfo.permissions || "-"}</code>
                </div>

                {/* Chmod 编辑器：SFTP v3 无可靠的 no-follow chmod，符号链接/特殊文件不开放 */}
                {canChmod && (
                  <div className={styles.fieldRow}>
                    <span className={styles.fieldLabel}>{t("fileManager.chmod")}</span>
                    <div className={styles.chmodEditor}>
                      <div className={styles.chmodRow}>
                        {chmodEditing ? (
                          <>
                            <input
                              className={`${styles.chmodInput} liquid-glass-input`}
                              type="text"
                              value={chmodValue}
                              maxLength={4}
                              aria-label={t("fileManager.chmod")}
                              onChange={(e) => {
                                setChmodValue(e.target.value.replace(/[^0-7]/g, ""));
                                setChmodError(null);
                              }}
                              onKeyDown={(e) => {
                                if (e.key === "Enter") handleChmodApply();
                              }}
                              autoFocus
                            />
                            <GlassButton
                              type="button"
                              size="sm"
                              className={styles.chmodBtn}
                              onClick={handleChmodApply}
                            >
                              {t("fileManager.apply")}
                            </GlassButton>
                            <GlassButton
                              type="button"
                              size="sm"
                              className={styles.chmodBtn}
                              onClick={cancelChmodEdit}
                            >
                              {t("fileManager.cancel")}
                            </GlassButton>
                          </>
                        ) : (
                          <>
                            <code className={styles.fieldValueMono}>{chmodValue}</code>
                            <GlassButton
                              type="button"
                              size="sm"
                              className={styles.chmodBtn}
                              onClick={() => setChmodEditing(true)}
                            >
                              {t("fileManager.edit")}
                            </GlassButton>
                          </>
                        )}
                      </div>
                      {chmodError && (
                        <span className={styles.chmodError} role="alert">{chmodError}</span>
                      )}
                    </div>
                  </div>
                )}
              </div>
            </>
          ) : null}
        </div>
      </div>
    </div>,
    document.body
  );
}
