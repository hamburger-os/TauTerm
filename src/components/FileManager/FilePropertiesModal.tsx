/**
 * 文件属性弹窗
 *
 * 显示远程文件/目录的详细元数据。
 * 主题样式参照 SettingsPage (glass-overlay + liquid-glass)。
 */
import { useCallback, useEffect, useState } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import Icon from "../common/Icon";
import { invoke } from "@tauri-apps/api/core";
import { formatTime } from "../../utils/format";
import type { SftpEntry } from "./types";
import { formatBytes } from "../../utils/format";
import styles from "./FilePropertiesModal.module.css";

export interface FileStatInfo {
  name: string;
  path: string;
  isDir: boolean;
  size: number;
  modified: number | null;
  permissions: string | null;
}

interface FilePropertiesModalProps {
  visible: boolean;
  entry: SftpEntry | null;
  statInfo: FileStatInfo | null;
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
  loading,
  onClose,
  sessionId,
  onChmodComplete,
}: FilePropertiesModalProps) {
  const { t } = useTranslation();

  // ── Chmod state ──
  const [chmodValue, setChmodValue] = useState("");
  const [chmodEditing, setChmodEditing] = useState(false);
  const [chmodError, setChmodError] = useState<string | null>(null);

  // Extract octal from permissions string (e.g. "-rw-r--r--" → "644")
  const getOctalFromPerms = useCallback((perms: string | null): string => {
    if (!perms || perms.length < 10) return "";
    let octal = 0;
    if (perms[1] === "r") octal += 0o400;
    if (perms[2] === "w") octal += 0o200;
    if (perms[3] === "x") octal += 0o100;
    if (perms[4] === "r") octal += 0o040;
    if (perms[5] === "w") octal += 0o020;
    if (perms[6] === "x") octal += 0o010;
    if (perms[7] === "r") octal += 0o004;
    if (perms[8] === "w") octal += 0o002;
    if (perms[9] === "x") octal += 0o001;
    return octal.toString(8).padStart(3, "0");
  }, []);

  // Reset chmod when statInfo changes
  useEffect(() => {
    if (statInfo?.permissions) {
      setChmodValue(getOctalFromPerms(statInfo.permissions));
      setChmodEditing(false);
      setChmodError(null);
    }
  }, [statInfo, getOctalFromPerms]);

  const handleChmodApply = useCallback(async () => {
    if (!/^[0-7]{3}$/.test(chmodValue)) {
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
      setChmodEditing(false);
      setChmodError(null);
      onChmodComplete?.();
    } catch (e) {
      setChmodError(String(e));
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
        await navigator.clipboard.writeText(statInfo.path);
      } catch {
        // Clipboard API may not be available
      }
    }
  }, [statInfo]);

  // ── Escape 关闭 ──
  useEffect(() => {
    if (!visible) return;
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, [visible, onClose]);

  if (!visible || !entry) return null;

  const isDir = statInfo?.isDir ?? entry.is_dir;
  const typeLabel = isDir ? t("fileManager.typeDir") : t("fileManager.typeFile");
  const typeEmoji = isDir ? "\u{1F4C1}" : "\u{1F4C4}";
  const name = statInfo?.name ?? entry.name;

  return createPortal(
    <div
      className={`${styles.overlay} glass-overlay`}
      onClick={handleOverlayClick}
    >
      <div className={`${styles.container} liquid-glass`}>
        {/* 标题栏 */}
        <div className={styles.header}>
          <span className={styles.headerTitle}>
            {typeEmoji} {name}
          </span>
          <button className={styles.closeBtn} onClick={onClose}>
            {t("common.close")}
          </button>
        </div>

        {/* 内容区 */}
        <div className={styles.body}>
          {loading ? (
            <div className={styles.loading}>{t("fileManager.loading")}</div>
          ) : statInfo ? (
            <>
              {/* 类型标签 */}
              <div className={styles.typeTag}>{typeLabel}</div>

              <div className={styles.fieldList}>
                {/* 完整路径 */}
                <div className={styles.fieldRow}>
                  <span className={styles.fieldLabel}>{t("fileManager.path")}</span>
                  <div className={styles.fieldValueRow}>
                    <code className={styles.fieldValuePath}>{statInfo.path}</code>
                    <button
                      className={styles.copyBtn}
                      onClick={handleCopyPath}
                      title={t("fileManager.copyPath")}
                    >
                      <Icon name="clipboard" size="sm" />
                    </button>
                  </div>
                </div>

                {/* 类型 */}
                <div className={styles.fieldRow}>
                  <span className={styles.fieldLabel}>{t("fileManager.type")}</span>
                  <span className={styles.fieldValue}>{typeLabel}</span>
                </div>

                {/* 大小 */}
                <div className={styles.fieldRow}>
                  <span className={styles.fieldLabel}>{t("fileManager.size")}</span>
                  <span className={styles.fieldValue}>
                    {isDir ? "-" : formatBytes(statInfo.size)}
                  </span>
                </div>

                {/* 修改时间 */}
                <div className={styles.fieldRow}>
                  <span className={styles.fieldLabel}>{t("fileManager.modified")}</span>
                  <span className={styles.fieldValue}>{formatTime(statInfo.modified)}</span>
                </div>

                {/* 权限 */}
                <div className={styles.fieldRow}>
                  <span className={styles.fieldLabel}>{t("fileManager.permissions")}</span>
                  <code className={styles.fieldValueMono}>{statInfo.permissions || "-"}</code>
                </div>

                {/* Chmod 编辑器 */}
                <div className={styles.fieldRow}>
                  <span className={styles.fieldLabel}>{t("fileManager.chmod")}</span>
                  <div className={styles.chmodRow}>
                    {chmodEditing ? (
                      <>
                        <input
                          className={styles.chmodInput}
                          type="text"
                          value={chmodValue}
                          maxLength={3}
                          onChange={(e) => {
                            setChmodValue(e.target.value.replace(/[^0-7]/g, ""));
                            setChmodError(null);
                          }}
                          onKeyDown={(e) => {
                            if (e.key === "Enter") handleChmodApply();
                            if (e.key === "Escape") {
                              setChmodEditing(false);
                              setChmodError(null);
                              if (statInfo?.permissions) {
                                setChmodValue(getOctalFromPerms(statInfo.permissions));
                              }
                            }
                          }}
                          autoFocus
                        />
                        <button className={styles.chmodBtn} onClick={handleChmodApply}>
                          {t("fileManager.apply")}
                        </button>
                        <button
                          className={styles.chmodBtn}
                          onClick={() => {
                            setChmodEditing(false);
                            setChmodError(null);
                            if (statInfo?.permissions) {
                              setChmodValue(getOctalFromPerms(statInfo.permissions));
                            }
                          }}
                        >
                          {t("fileManager.cancel")}
                        </button>
                      </>
                    ) : (
                      <>
                        <code className={styles.fieldValueMono}>{chmodValue}</code>
                        <button
                          className={styles.chmodBtn}
                          onClick={() => setChmodEditing(true)}
                        >
                          {t("fileManager.edit")}
                        </button>
                      </>
                    )}
                  </div>
                  {chmodError && <span className={styles.chmodError}>{chmodError}</span>}
                </div>
              </div>
            </>
          ) : null}
        </div>
      </div>
    </div>,
    document.body
  );
}
