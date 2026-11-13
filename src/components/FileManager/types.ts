export interface SftpEntry {
  name: string;
  path: string;
  is_dir: boolean;
  entry_type?: 'file' | 'directory' | 'symlink' | 'fifo' | 'socket' | 'block_device' | 'char_device' | 'other';
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
