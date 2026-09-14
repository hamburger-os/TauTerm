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

export type XmodemReceiveCheckMode = "auto" | "crc16" | "checksum";
export type ZmodemCrcPolicy = "auto" | "crc16" | "crc32-required";
export type ZmodemReceiveCrcCapability = "auto" | "crc16-only";
export type ModemBlockSize = 128 | 1024;
export type ZmodemMaxBlockSize = 1024 | 2048 | 4096 | 8192;

/** YMODEM 的块大小属于发送方；接收方按标准流程主动请求 CRC16。 */
export interface YmodemTransferConfig {
  protocol: "ymodem";
  send: { blockSize: ModemBlockSize };
}

/** XMODEM 将发送块大小与接收校验请求分开建模。 */
export interface XmodemTransferConfig {
  protocol: "xmodem";
  send: { blockSize: ModemBlockSize };
  receive: { checkMode: XmodemReceiveCheckMode };
}

/** ZMODEM 发送策略与接收方宣告的 CRC 能力分别建模。 */
export interface ZmodemTransferConfig {
  protocol: "zmodem";
  send: {
    crcPolicy: ZmodemCrcPolicy;
    maxBlockSize: ZmodemMaxBlockSize;
  };
  receive: { crcCapability: ZmodemReceiveCrcCapability };
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
      send: { blockSize: 1024 },
    },
  },
  xmodem: {
    type: "xmodem",
    i18nKey: "transfer.protocols.xmodem.name",
    icon: "package",
    defaultConfig: {
      protocol: "xmodem",
      send: { blockSize: 128 },
      receive: { checkMode: "auto" },
    },
  },
  zmodem: {
    type: "zmodem",
    i18nKey: "transfer.protocols.zmodem.name",
    icon: "package",
    defaultConfig: {
      protocol: "zmodem",
      send: { crcPolicy: "auto", maxBlockSize: 8192 },
      receive: { crcCapability: "auto" },
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

export type SendProtocolOptions =
  | { protocol: "ymodem"; blockSize: ModemBlockSize }
  | { protocol: "xmodem"; blockSize: ModemBlockSize }
  | { protocol: "zmodem"; crcPolicy: ZmodemCrcPolicy; maxBlockSize: ZmodemMaxBlockSize }
  | { protocol: "sftp" };

export type ReceiveProtocolOptions =
  | { protocol: "ymodem" }
  | { protocol: "xmodem"; checkMode: XmodemReceiveCheckMode }
  | { protocol: "zmodem"; crcCapability: ZmodemReceiveCrcCapability }
  | { protocol: "sftp" };

/** 前端 → 后端发送命令：协议专有设置只存在于 role-aware protocolOptions。 */
export interface FileTransferSendRequest {
  sessionId: string;
  protocolOptions: SendProtocolOptions;
  filePaths: string[];
  remoteDir?: string;
  overwritePolicy?: OverwritePolicy;
}

/** 前端 → 后端接收命令：不会复用发送方配置。 */
export interface FileTransferReceiveRequest {
  sessionId: string;
  protocolOptions: ReceiveProtocolOptions;
  downloadDir: string;
  remotePaths: string[];
  destinationPaths?: string[];
  overwritePolicy?: OverwritePolicy;
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
