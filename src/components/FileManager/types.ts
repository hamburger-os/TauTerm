/** 远端目录项类型（与后端 `SftpEntryType` 的 snake_case 序列化对齐）。 */
export type SftpEntryType =
  | 'file'
  | 'directory'
  | 'symlink'
  | 'fifo'
  | 'socket'
  | 'block_device'
  | 'char_device'
  | 'other';

/**
 * 远端目录项（`sftp_list_dir_cmd` / `sftp_stat_cmd` 的结果）。
 *
 * `entry_type` 是目录项类型的唯一真源，不再保留“是否目录”这类派生布尔字段。
 * 类型判定统一走 `entryKind.ts`，不要在组件里直接比较该字段。
 */
export interface SftpEntry {
  name: string;
  path: string;
  entry_type: SftpEntryType;
  size: number;
  accessed: number | null;
  modified: number | null;
  permissions: string | null;
}

/** `sftp_list_dir_cmd` 的结果：后端规范化后的路径 + 目录项。 */
export interface SftpListing {
  path: string;
  entries: SftpEntry[];
}

/** `ssh_sftp_availability_cmd` 的结果（每个 SSH 连接探测一次并缓存）。 */
export interface SftpAvailability {
  available: boolean;
  reason: string | null;
}

export type PromptMode = 'newFile' | 'newFolder' | 'rename';

export type SortField = 'name' | 'size' | 'modified';
export type SortDirection = 'asc' | 'desc';
export type {
  OverwritePolicy,
  TransferFinishedPayload,
  TransferStartedPayload,
  TransferStartAck,
  UnifiedTransferProgressPayload as UnifiedProgressPayload,
} from '../../types/transfer';
