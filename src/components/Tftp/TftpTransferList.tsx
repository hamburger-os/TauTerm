/**
 * TFTP 传输列表
 *
 * 显示所有活跃/历史传输的进度和状态。
 * 设计对齐 YMODEM PerFileList 的 Mini-Card 模式。
 */
import { useTranslation } from "react-i18next";
import type { TransferState } from "./TftpSessionView";
import ProgressBar from "../FileTransfer/progress/ProgressBar";
import Icon from "../common/Icon";
import type { IconName } from "../common/Icon";
import styles from "./TftpSessionView.module.css";

function formatSize(bytes: number): string {
  if (bytes === 0) return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  const i = Math.floor(Math.log(bytes) / Math.log(1024));
  return `${(bytes / Math.pow(1024, i)).toFixed(1)} ${units[i]}`;
}

function formatSpeed(bps: number): string {
  if (bps <= 0) return "";
  return `${formatSize(bps)}/s`;
}

function getStatusIcon(status: string): IconName {
  switch (status) {
    case "transferring": return "transfer-progress";
    case "completed":     return "check-circle";
    case "failed":        return "cross-circle";
    case "cancelled":     return "close-circle";
    case "pending":       return "hourglass";
    default:              return "info";
  }
}

interface Props {
  transfers: TransferState[];
}

export default function TftpTransferList({ transfers }: Props) {
  const { t } = useTranslation();

  if (transfers.length === 0) {
    return (
      <div className={`${styles.panel} liquid-glass-card`}>
        <h3>{t("tftp.transfers")}</h3>
        <p className={styles.empty}>{t("tftp.noTransfers")}</p>
      </div>
    );
  }

  return (
    <div className={`${styles.panel} liquid-glass-card`}>
      <h3>{t("tftp.transfers")}</h3>
      <div className={styles.transferTable}>
        {transfers.map((tf) => {
          const percent =
            tf.totalBytes > 0
              ? Math.min(100, Math.round((tf.bytesTransferred / tf.totalBytes) * 100))
              : 0;
          const isTransferring = tf.status === "transferring";

          return (
            <div key={tf.id} className={styles.transferRow}>
              {/* 状态图标 */}
              <span className={styles.tfIconCell}>
                <Icon name={getStatusIcon(tf.status)} size="sm" />
              </span>

              {/* 文件信息区 */}
              <div className={styles.tfFileInfo}>
                <span title={tf.filename} className={styles.tfFilename}>
                  {tf.filename}
                </span>

                {/* 元信息行：方向 + 角色 + 远端地址 */}
                <span className={styles.tfMetaLine}>
                  <Icon
                    name={tf.direction === "download" ? "download" : "upload"}
                    size="sm"
                  />
                  {" "}
                  {tf.isServer ? t("tftp.serverIndicator") : t("tftp.clientIndicator")}
                  {tf.remoteAddr ? ` · ${tf.remoteAddr}` : ""}
                </span>

                {/* 进度条区域 —— 始终保留高度 */}
                <div className={styles.tfProgressSlot}>
                  {isTransferring && (
                    <ProgressBar
                      percent={percent}
                      height={3}
                      indeterminate={tf.totalBytes === 0}
                    />
                  )}
                </div>
              </div>

              {/* 字节数 + 速度 + CRC32 */}
              <span className={styles.tfFileSize}>
                {tf.status === "pending"
                  ? "—"
                  : formatSize(tf.bytesTransferred)}
                {tf.status === "completed" && tf.avgBytesPerSecond > 0 && (
                  <span className={styles.tfChecksum}>{formatSpeed(tf.avgBytesPerSecond)}</span>
                )}
                {isTransferring && tf.bytesPerSecond > 0 && (
                  <span className={styles.tfChecksum}>{formatSpeed(tf.bytesPerSecond)}</span>
                )}
                {tf.checksum && (
                  <span className={styles.tfChecksum}>CRC32:{tf.checksum}</span>
                )}
              </span>
            </div>
          );
        })}
      </div>
    </div>
  );
}
