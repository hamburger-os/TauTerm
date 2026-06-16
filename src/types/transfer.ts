/** 传输方向 */
export type TransferDirection = "send" | "receive";

/** 传输状态 */
export type TransferStatus = "idle" | "transferring" | "completed" | "failed" | "cancelled";

/** 传输进度信息 */
export interface TransferProgress {
  file_name: string;
  bytes_transferred: number;
  total_bytes: number;
  direction: TransferDirection;
}

/** 传输历史记录 */
export interface TransferHistoryItem {
  id: string;
  file_name: string;
  direction: TransferDirection;
  size: number;
  status: TransferStatus;
  timestamp: number;
  error?: string;
}
