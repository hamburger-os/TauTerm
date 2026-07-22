export interface SftpEntry {
  name: string;
  path: string;
  is_dir: boolean;
  size: number;
  modified: number | null;
  permissions: string | null;
}

export type PromptMode = 'newFile' | 'newFolder' | 'rename';

export type SortField = 'name' | 'size' | 'modified';
export type SortDirection = 'asc' | 'desc';

/** 统一文件传输进度事件载荷（对应后端 `file-transfer:progress` emit） */
export interface UnifiedProgressPayload {
  session_id: string;
  protocol: string;
  file_name: string;
  bytes_done: number;
  bytes_total: number;
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

/** 统一文件传输完成事件载荷（对应后端 `file-transfer:finished` emit） */
export interface TransferFinishedPayload {
  session_id: string;
  protocol: string;
  success: boolean;
  error?: string;
}

export interface ReadHeadResult {
  data: number[];
  total_size: number;
}
