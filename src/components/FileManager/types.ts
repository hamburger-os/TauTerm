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

export interface SftpProgressPayload {
  session_id: string;
  file_name: string;
  direction: 'upload' | 'download';
  bytes_done: number;
  bytes_total: number;
}

export interface ReadHeadResult {
  data: number[];
  total_size: number;
}
