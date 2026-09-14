import type { IconName } from "../components/common/Icon";

/** 协议标识 */
export type ProtocolType = "ymodem" | "xmodem" | "zmodem" | "sftp";
/** 串口内联传输协议（下拉框选项，不含 SFTP） */
export const PROTOCOL_TYPES: ProtocolType[] = ["ymodem", "xmodem", "zmodem"];

/** 传输方向 */
export type TransferDirection = "send" | "receive";

/** 目标冲突策略。由 UI 解析用户意图，后端负责安全提交。 */
export type OverwritePolicy = "replace" | "skip" | "keep-both";

/** 传输启动确认。transfer_id 是取消、进度和终态匹配的唯一任务身份。 */
export interface TransferStartAck {
  transfer_id: string;
}

/** 传输状态 */
export type TransferStatus =
  | "idle"
  | "transferring"
  | "completed"
  | "failed"
  | "cancelled";

/** 单个文件在批次中的状态 */
export type FileTransferState =
  | "pending"
  | "transferring"
  | "completed"
  | "failed"
  | "skipped";

// ── Protocol Config Interfaces ────────────────────────────

/** YModem 当前唯一可由用户强制选择的参数是发送块大小；CRC/Checksum/G 由对端握手协商。 */
export interface YmodemTransferConfig {
  protocol: "ymodem";
  blockSize: 128 | 1024;
}

/** XModem 变体、校验方式和启动字符由双方握手自动协商。 */
export interface XmodemTransferConfig {
  protocol: "xmodem";
}

/** ZModem 能力由协议握手自动协商；当前不暴露不会实际生效的伪配置。 */
export interface ZmodemTransferConfig {
  protocol: "zmodem";
}

/** SFTP 传输配置（文件管理器拥有真实远端目录状态）。 */
export interface SftpTransferConfig {
  protocol: "sftp";
  remotePath: string;
}

export type TransferConfig =
  | YmodemTransferConfig
  | XmodemTransferConfig
  | ZmodemTransferConfig
  | SftpTransferConfig;

// ── Protocol Registry ─────────────────────────────────────

export interface ProtocolMeta {
  type: ProtocolType;
  i18nKey: string;
  icon: IconName;
  defaultConfig: TransferConfig;
}

export const PROTOCOL_REGISTRY: Record<ProtocolType, ProtocolMeta> = {
  ymodem: {
    type: "ymodem",
    i18nKey: "transfer.protocols.ymodem.name",
    icon: "package",
    defaultConfig: {
      protocol: "ymodem",
      blockSize: 1024,
    },
  },
  xmodem: {
    type: "xmodem",
    i18nKey: "transfer.protocols.xmodem.name",
    icon: "package",
    defaultConfig: {
      protocol: "xmodem",
    },
  },
  zmodem: {
    type: "zmodem",
    i18nKey: "transfer.protocols.zmodem.name",
    icon: "package",
    defaultConfig: {
      protocol: "zmodem",
    },
  },
  sftp: {
    type: "sftp",
    i18nKey: "transfer.protocols.sftp.name",
    icon: "folder",
    defaultConfig: {
      protocol: "sftp",
      remotePath: "/",
    },
  },
};

// ── Transfer Commands ─────────────────────────────────────

/** 前端 → 后端发送命令的精确结构；协议专有参数只保留真实生效的 YModem blockSize。 */
export interface FileTransferSendRequest {
  sessionId: string;
  protocol: string;
  filePaths: string[];
  remoteDir?: string;
  overwritePolicy?: OverwritePolicy;
  blockSize?: 128 | 1024;
}

/** 前端 → 后端接收命令的精确结构。 */
export interface FileTransferReceiveRequest {
  sessionId: string;
  protocol: string;
  downloadDir: string;
  remotePaths: string[];
  destinationPaths?: string[];
  overwritePolicy?: OverwritePolicy;
  blockSize?: 128 | 1024;
}

// ── Transfer Events ───────────────────────────────────────

/** 后端 file-transfer:started 事件。 */
export interface TransferStartedPayload {
  session_id: string;
  transfer_id: string;
  protocol: string;
  direction: TransferDirection;
}

/** 进度流中的显式事件类型；不使用互相冲突的布尔标记组合。 */
export type TransferProgressKind =
  | "file_start"
  | "progress"
  | "file_complete"
  | "batch_complete";

/** 后端 file-transfer:progress 统一事件。 */
export interface UnifiedTransferProgressPayload {
  session_id: string;
  transfer_id: string;
  kind: TransferProgressKind;
  protocol: string;
  file_name: string;
  bytes_done: number;
  bytes_total: number;
  bytes_per_second: number | null;
  file_index: number;
  total_files: number;
  aggregate_bytes: number;
  aggregate_total: number;
  direction: TransferDirection;
  file_success: boolean | null;
  file_error: string | null;
}

/** 批次中单个文件的结果。 */
export interface BatchFileResult {
  file_name: string;
  status: "completed" | "failed" | "skipped";
  size: number;
  error?: string | null;
}

/** 后端 file-transfer:finished 终态事件；每个字段都是任务终态协议的一部分。 */
export interface TransferFinishedPayload {
  session_id: string;
  transfer_id: string;
  protocol: string;
  success: boolean;
  cancelled: boolean;
  error: string | null;
  results: BatchFileResult[] | null;
}

// ── Frontend State Types ──────────────────────────────────

/** 传输历史记录 */
export interface TransferHistoryItem {
  id: string;
  file_name: string;
  direction: TransferDirection;
  size: number;
  status: TransferStatus;
  timestamp: number;
  error?: string;
  protocol: ProtocolType | "unknown";
}

/** 批次文件条目（前端 UI 状态） */
export interface BatchFileEntry {
  fileName: string;
  status: FileTransferState;
  bytesTransferred: number;
  totalBytes: number;
  error?: string;
}
