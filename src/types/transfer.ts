/** 协议标识 */
export type ProtocolType = "ymodem" | "xmodem" | "zmodem";
export const PROTOCOL_TYPES: ProtocolType[] = ["ymodem", "xmodem", "zmodem"];

/** 传输方向 */
export type TransferDirection = "send" | "receive";

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

export interface YmodemTransferConfig {
  protocol: "ymodem";
  /** 块大小（字节），YModem 标准为 1024 */
  blockSize: 128 | 1024;
  /** 校验模式 */
  checksumMode: "crc16" | "crc32";
}

export interface XmodemTransferConfig {
  protocol: "xmodem";
  /** 块大小：标准 XModem 128 字节，XModem-1K 1024 字节 */
  blockSize: 128 | 1024;
  /** 校验模式 */
  checksumMode: "checksum" | "crc16";
  /** 启动字符：NAK（标准）或 'C'（CRC 模式接收方） */
  initChar: "nak" | "crc";
}

export interface ZmodemTransferConfig {
  protocol: "zmodem";
  /** 滑动窗口大小 1-16 */
  windowSize: number;
  /** 断点续传 */
  resumeEnabled: boolean;
  /** ZMODEM-90 压缩 */
  compressionEnabled: boolean;
  /** 流式传输（未知文件大小） */
  streamingMode: boolean;
}

export type TransferConfig =
  | YmodemTransferConfig
  | XmodemTransferConfig
  | ZmodemTransferConfig;

// ── Protocol Registry ─────────────────────────────────────

export interface ProtocolMeta {
  type: ProtocolType;
  i18nKey: string; // e.g. "transfer.protocols.ymodem.name"
  icon: string; // e.g. "📦"
  defaultConfig: TransferConfig;
}

export const PROTOCOL_REGISTRY: Record<ProtocolType, ProtocolMeta> = {
  ymodem: {
    type: "ymodem",
    i18nKey: "transfer.protocols.ymodem.name",
    icon: "📦",
    defaultConfig: {
      protocol: "ymodem",
      blockSize: 1024,
      checksumMode: "crc16",
    },
  },
  xmodem: {
    type: "xmodem",
    i18nKey: "transfer.protocols.xmodem.name",
    icon: "📡",
    defaultConfig: {
      protocol: "xmodem",
      blockSize: 128,
      checksumMode: "checksum",
      initChar: "nak",
    },
  },
  zmodem: {
    type: "zmodem",
    i18nKey: "transfer.protocols.zmodem.name",
    icon: "⚡",
    defaultConfig: {
      protocol: "zmodem",
      windowSize: 4,
      resumeEnabled: true,
      compressionEnabled: false,
      streamingMode: false,
    },
  },
};

// ── Transfer Events ───────────────────────────────────────

/** 传输进度信息 */
export interface TransferProgress {
  file_name: string;
  bytes_transferred: number;
  total_bytes: number;
  direction: TransferDirection;
  /** 当前文件在批次中的索引（0-based） */
  file_index?: number;
  /** 批次中文件总数 */
  total_files?: number;
  /** 聚合已传输字节（已完成文件 + 当前文件进度） */
  aggregate_bytes_transferred?: number;
  /** 聚合总字节 */
  aggregate_total_bytes?: number;
}

/** 文件开始事件 */
export interface FileStartEvent {
  file_name: string;
  file_index: number;
  total_files: number;
  file_size: number;
}

/** 文件完成事件 */
export interface FileCompleteEvent {
  file_name: string;
  file_index: number;
  total_files: number;
  bytes_transferred: number;
  success: boolean;
  error?: string | null;
}

/** 批次中单个文件的结果 */
export interface BatchFileResult {
  file_name: string;
  status: "completed" | "failed" | "skipped";
  size: number;
  error?: string | null;
}

/** 传输完成事件 */
export interface TransferCompleteEvent {
  success: boolean;
  files_completed?: number;
  files_failed?: number;
  files_skipped?: number;
  message?: string;
  results?: BatchFileResult[];
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
  /** 使用的协议 */
  protocol: ProtocolType;
}

/** 批次文件条目（前端 UI 状态） */
export interface BatchFileEntry {
  fileName: string;
  status: FileTransferState;
  bytesTransferred: number;
  totalBytes: number;
  error?: string;
}

/** 历史记录过滤器 */
export interface HistoryFilter {
  protocol: ProtocolType | "all";
  direction: TransferDirection | "all";
  status: TransferStatus | "all";
}

export const DEFAULT_HISTORY_FILTER: HistoryFilter = {
  protocol: "all",
  direction: "all",
  status: "all",
};
