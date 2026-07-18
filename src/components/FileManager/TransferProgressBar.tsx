/**
 * 传输进度条组件
 *
 * 固定在文件管理器底部的传输进度显示条。
 * 支持上传/下载方向、进度百分比、速率计算和自动隐藏。
 */
import { useEffect, useState, useRef } from "react";
import { useTranslation } from "react-i18next";
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
  onClose: () => void;
}

export default function TransferProgressBar({
  visible,
  fileName,
  direction,
  percent,
  finished,
  speed,
  onClose,
}: TransferProgressBarProps) {
  const { t } = useTranslation();
  const [autoHiding, setAutoHiding] = useState(false);
  const hideTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // 完成（100%）或取消/失败（finished 且 percent < 100）后自动隐藏。
  // 完成延迟 2s，取消/失败延迟由父组件控制（通常 1.5s）。
  useEffect(() => {
    const shouldAutoHide = percent >= 100 || finished;
    if (shouldAutoHide && visible && !autoHiding) {
      setAutoHiding(true);
      const delay = percent >= 100 ? 2000 : 1500;
      hideTimerRef.current = setTimeout(() => {
        onClose();
        setAutoHiding(false);
      }, delay);
    }
    if (!shouldAutoHide) {
      setAutoHiding(false);
    }
    return () => {
      if (hideTimerRef.current) {
        clearTimeout(hideTimerRef.current);
      }
    };
  }, [percent, visible, onClose, autoHiding, finished]);

  if (!visible) return null;

  const directionIcon = direction === "upload" ? "⬆" : "⬇";
  const clampedPercent = Math.min(100, Math.max(0, percent));

  return (
    <div className={styles.bar}>
      {/* 左侧：方向图标 + 文件名 */}
      <span className={styles.left}>
        <span className={styles.dirIcon}>{directionIcon}</span>
        <span className={styles.fileName} title={fileName}>
          {truncateName(fileName)}
        </span>
      </span>

      {/* 中间：进度条 */}
      <div className={styles.progressTrack}>
        <div
          className={styles.progressFill}
          style={{ width: `${clampedPercent}%` }}
        />
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
