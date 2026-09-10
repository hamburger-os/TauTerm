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

export interface ReadHeadResult {
  data: number[];
  total_size: number;
}
