export interface SftpEntry {
  name: string;
  path: string;
  is_dir: boolean;
  size: number;
  accessed: number | null;
  modified: number | null;
  permissions: string | null;
}

export type PromptMode = 'newFile' | 'newFolder' | 'rename';

export type SortField = 'name' | 'size' | 'modified';
export type SortDirection = 'asc' | 'desc';
export type { OverwritePolicy, TransferStartAck } from '../../types/transfer';

/** 统一文件传输进度事件载荷（对应后端 `file-transfer:progress` emit） */
export interface UnifiedProgressPayload {
  session_id: string;
  transfer_id: string;
  protocol: string;
  file_name: string;
  bytes_done: number;
  bytes_total: number;
  bytes_per_second: number | null;
  file_index: number;
  total_files: number;
  aggregate_bytes: number;
  aggregate_total: number;
  direction: 'send' | 'receive';
  is_file_start: boolean;
  is_file_complete: boolean;
  file_success: boolean | null;
  file_error: string | null;
  is_batch_complete: boolean;
}

/** 统一文件传输启动事件载荷（对应后端 `file-transfer:started` emit） */
export interface TransferStartedPayload {
  session_id: string;
  transfer_id: string;
  protocol: string;
  direction: 'send' | 'receive';
}

/** 统一文件传输完成事件载荷（对应后端 `file-transfer:finished` emit） */
export interface TransferFinishedPayload {
  session_id: string;
  transfer_id?: string;
  protocol?: string;
  success: boolean;
  cancelled?: boolean;
  error?: string | null;
}

export interface ReadHeadResult {
  data: number[];
  total_size: number;
}
