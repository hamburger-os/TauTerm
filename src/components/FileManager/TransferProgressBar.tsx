/**
 * 传输进度条组件（纯视觉层）
 *
 * 固定在文件管理器底部的传输进度显示条。
 * 支持上传/下载方向、进度百分比、速率计算和批量聚合进度。
 *
 * **不包含任何自动隐藏逻辑**——所有状态和时序由父组件
 * （`useSftpProgress` Hook）统一管理。× 按钮直接调用 `onClose`。
 */
import { useTranslation } from "react-i18next";
import Icon from "../common/Icon";
import styles from "./TransferProgressBar.module.css";

// ── Helpers ────────────────────────────────────────────

function formatSpeed(bytesPerSec: number): string {
  if (bytesPerSec <= 0) return "0 KB/s";
  if (bytesPerSec < 1024) return `${bytesPerSec.toFixed(0)} B/s`;
  if (bytesPerSec < 1024 * 1024) return `${(bytesPerSec / 1024).toFixed(1)} KB/s`;
  return `${(bytesPerSec / (1024 * 1024)).toFixed(1)} MB/s`;
}

function truncateName(name: string, maxLen: number = 30): string {
  if (name.length <= maxLen) return name;
  return name.substring(0, maxLen - 3) + "...";
}

// ── Component ──────────────────────────────────────────

interface TransferProgressBarProps {
  visible: boolean;
  fileName: string;
  direction: "upload" | "download";
  percent: number;
  /** 传输是否已结束（完成/取消/失败）—— false 表示进行中，× 按钮将触发取消 */
  finished: boolean;
  /** EMA 平滑后的瞬时速度（字节/秒），由父组件通过进度事件计算 */
  speed: number;
  /** 当前文件索引 (0-based, 仅多文件批次使用) */
  fileIndex?: number;
  /** 批次文件总数 (默认 1) */
  totalFiles?: number;
  /** 批次聚合百分比 (仅多文件批次使用) */
  aggregatePercent?: number;
  onClose: () => void;
}

export default function TransferProgressBar({
  visible,
  fileName,
  direction,
  percent,
  finished,
  speed,
  fileIndex = 0,
  totalFiles = 1,
  aggregatePercent,
  onClose,
}: TransferProgressBarProps) {
  const { t } = useTranslation();

  if (!visible) return null;

  const directionIcon = direction === "upload" ? "upload" : "download";
  const clampedPercent = Math.min(100, Math.max(0, percent));
  const isBatch = totalFiles > 1;
  const aggPercent = aggregatePercent ?? percent;
  const clampedAgg = Math.min(100, Math.max(0, aggPercent));

  return (
    <div className={styles.bar}>
      {/* 左侧：方向图标 + 文件名 */}
      <span className={styles.left}>
        <Icon name={directionIcon} size="sm" className={styles.dirIcon} />
        <span className={styles.fileName} title={fileName}>
          {truncateName(fileName)}
          {isBatch && (
            <span className={styles.batchLabel}>
              {" "}({fileIndex + 1}/{totalFiles})
            </span>
          )}
        </span>
      </span>

      {/* 中间：进度条(s) */}
      <div className={styles.progressArea}>
        <div className={styles.progressTrack}>
          <div
            className={styles.progressFill}
            style={{ width: `${clampedPercent}%` }}
          />
        </div>
        {isBatch && (
          <div className={`${styles.progressTrack} ${styles.aggregateTrack}`}>
            <div
              className={`${styles.progressFill} ${styles.aggregateFill}`}
              style={{ width: `${clampedAgg}%` }}
            />
          </div>
        )}
      </div>

      {/* 右侧：百分比 + 速率 + 关闭 */}
      <span className={styles.right}>
        <span className={styles.percentText}>
          {clampedPercent.toFixed(0)}%
        </span>
        <span className={styles.speedText}>{formatSpeed(speed)}</span>
        <button
          className={styles.closeBtn}
          onClick={onClose}
          title={finished ? t("common.close") : t("fileManager.cancelTransfer")}
        >
          X
        </button>
      </span>
    </div>
  );
}
