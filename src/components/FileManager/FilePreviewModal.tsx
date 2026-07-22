/**
 * 文本文件预览弹窗
 *
 * 将远端文本文件下载到临时目录后在只读编辑器中展示。
 * 主题样式参照 SettingsPage (glass-overlay + liquid-glass)。
 *
 * ≤10MB 直接加载，>10MB 用户确认后再加载。
 */
import { useCallback, useEffect } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import Icon from "../common/Icon";
import { formatBytes } from "../../utils/format";
import styles from "./FilePreviewModal.module.css";

// ── Component ──────────────────────────────────────────

interface FilePreviewModalProps {
  visible: boolean;
  fileName: string;
  content: string | null;
  loading: boolean;
  error: string | null;
  fileSize: number;
  onClose: () => void;
}

export default function FilePreviewModal({
  visible,
  fileName,
  content,
  loading,
  error,
  fileSize,
  onClose,
}: FilePreviewModalProps) {
  const { t } = useTranslation();

  const handleOverlayClick = useCallback(
    (e: React.MouseEvent) => {
      if (e.target === e.currentTarget) onClose();
    },
    [onClose]
  );

  // ── Escape 关闭 ──
  useEffect(() => {
    if (!visible) return;
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, [visible, onClose]);

  if (!visible) return null;

  const lineCount = content ? content.split("\n").length : 0;

  return createPortal(
    <div
      className={`${styles.overlay} glass-overlay`}
      onClick={handleOverlayClick}
    >
      <div className={`${styles.container} liquid-glass`}>
        {/* 标题栏 */}
        <div className={styles.header}>
          <span className={styles.headerTitle}>{fileName}</span>
          <button className={styles.closeBtn} onClick={onClose}>
            <Icon name="close" size="md" />
          </button>
        </div>

        {/* 内容区 */}
        <div className={styles.body}>
          {loading && (
            <div className={styles.loading}>{t("fileManager.loading")}</div>
          )}
          {error && (
            <div className={styles.error}>{error}</div>
          )}
          {!loading && !error && content !== null && (
            <pre className={styles.previewArea}>{content}</pre>
          )}
        </div>

        {/* 底部状态栏 */}
        {content !== null && !loading && !error && (
          <div className={styles.statusBar}>
            <span className={styles.statusItem}>{formatBytes(fileSize)}</span>
            <span className={styles.statusItem}>
              {t("fileManager.lines", { count: lineCount })}
            </span>
          </div>
        )}
      </div>
    </div>,
    document.body
  );
}
