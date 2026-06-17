import { useState } from "react";
import type { TransferConfig } from "../../types/transfer";
import { PROTOCOL_REGISTRY } from "../../types/transfer";
import { useTransfer } from "../../context/TransferContext";
import ConfigColumn from "./columns/ConfigColumn";
import ProgressColumn from "./columns/ProgressColumn";
import HistoryColumn from "./columns/HistoryColumn";
import styles from "./FileTransferPanel.module.css";

interface FileTransferPanelProps {
  isConnected: boolean;
  onSendFiles: () => void;
  onReceiveFiles: () => void;
  onCancel: () => void;
}

/**
 * 文件传输面板 — 三列编排组件
 *
 * 左列(ConfigColumn)：协议选择、参数配置、操作按钮
 * 中列(ProgressColumn)：聚合进度条、逐文件列表、错误汇总
 * 右列(HistoryColumn)：历史记录过滤器 + 列表
 */
export default function FileTransferPanel({
  isConnected,
  onSendFiles,
  onReceiveFiles,
  onCancel,
}: FileTransferPanelProps) {
  const { state, clearError, clearHistory } = useTransfer();
  const {
    status,
    error,
    history,
    batchFiles,
    aggregateBytesTransferred,
    aggregateTotalBytes,
    currentFileIndex,
    totalFiles,
  } = state;

  // Local protocol config state
  const [config, setConfig] = useState<TransferConfig>(
    PROTOCOL_REGISTRY.ymodem.defaultConfig,
  );

  const isTransferring = status === "transferring";
  const canTransfer = isConnected && !isTransferring;
  const batchEntries = Object.values(batchFiles);

  return (
    <div className={styles.panel}>
      <ConfigColumn
        config={config}
        onConfigChange={setConfig}
        isConnected={isConnected}
        isTransferring={isTransferring}
        canTransfer={canTransfer}
        batchCount={batchEntries.length}
        aggregateTotal={aggregateTotalBytes}
        onSend={onSendFiles}
        onReceive={onReceiveFiles}
        onCancel={onCancel}
      />

      <ProgressColumn
        isTransferring={isTransferring}
        batchEntries={batchEntries}
        currentFileIndex={currentFileIndex}
        totalFiles={totalFiles}
        aggregateBytesTransferred={aggregateBytesTransferred}
        aggregateTotalBytes={aggregateTotalBytes}
        error={error}
        onClearError={clearError}
      />

      <HistoryColumn items={history} onClear={clearHistory} />
    </div>
  );
}
