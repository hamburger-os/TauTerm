import { useState, useCallback, useEffect, useMemo } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { save } from '@tauri-apps/plugin-dialog';
import { SftpEntry, PromptMode, SortField, SortDirection } from '../types';

export interface UseFileManagerReturn {
  currentPath: string;
  entries: SftpEntry[];
  loading: boolean;
  error: string | null;
  breadcrumbSegments: { name: string; path: string }[];
  promptMode: PromptMode | null;
  promptValue: string;
  promptTarget: SftpEntry | null;
  sortField: SortField;
  sortDirection: SortDirection;

  loadDirectory: (path: string) => Promise<void>;
  navigateTo: (path: string) => void;
  goUp: () => void;
  refresh: () => Promise<void>;
  uploadFile: (localPath: string, remotePath: string) => Promise<void>;
  downloadFiles: (entries: SftpEntry[]) => Promise<void>;
  downloadDirectory: (remoteDir: string, localDir: string) => Promise<void>;
  deleteEntries: (entries: SftpEntry[]) => Promise<string[]>;
  renameEntry: (entry: SftpEntry, newName: string) => Promise<void>;
  createFile: (name: string) => Promise<void>;
  createFolder: (name: string) => Promise<void>;
  setPromptMode: (mode: PromptMode | null) => void;
  setPromptValue: (val: string) => void;
  setPromptTarget: (entry: SftpEntry | null) => void;
  setSortField: (field: SortField) => void;
  clearError: () => void;
}

function sortEntries(
  list: SftpEntry[],
  field: SortField,
  direction: SortDirection
): SftpEntry[] {
  const dirs = list.filter(e => e.is_dir);
  const files = list.filter(e => !e.is_dir);

  const cmp = (a: SftpEntry, b: SftpEntry): number => {
    let result: number;
    switch (field) {
      case 'name':
        result = a.name.localeCompare(b.name, undefined, { sensitivity: 'base' });
        break;
      case 'size':
        result = a.size - b.size;
        break;
      case 'modified': {
        const ma = a.modified ?? null;
        const mb = b.modified ?? null;
        if (ma === null && mb === null) result = 0;
        else if (ma === null) result = 1;
        else if (mb === null) result = -1;
        else result = ma - mb;
        break;
      }
    }
    return direction === 'desc' ? -result : result;
  };

  dirs.sort(cmp);
  files.sort(cmp);
  return [...dirs, ...files];
}

export function useFileManager(
  sessionId: string,
  isConnected: boolean
): UseFileManagerReturn {
  const [currentPath, setCurrentPath] = useState('/');
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [rawEntries, setRawEntries] = useState<SftpEntry[]>([]);
  const [sortField, setSortField] = useState<SortField>('name');
  const [sortDirection, setSortDirection] = useState<SortDirection>('asc');
  const [promptMode, setPromptMode] = useState<PromptMode | null>(null);
  const [promptValue, setPromptValue] = useState('');
  const [promptTarget, setPromptTarget] = useState<SftpEntry | null>(null);

  // ── Sorted entries (dirs first, then by sortField/direction) ──
  const entries = useMemo(
    () => sortEntries(rawEntries, sortField, sortDirection),
    [rawEntries, sortField, sortDirection]
  );

  // ── Breadcrumb segments ──
  const breadcrumbSegments = useMemo((): { name: string; path: string }[] => {
    if (currentPath === '/') {
      return [{ name: '/', path: '/' }];
    }
    const segments = currentPath.split('/').filter(Boolean);
    return [
      { name: '/', path: '/' },
      ...segments.map((seg, i) => ({
        name: seg,
        path: '/' + segments.slice(0, i + 1).join('/'),
      })),
    ];
  }, [currentPath]);

  // ── Load directory ──
  const loadDirectory = useCallback(
    async (path: string) => {
      if (!isConnected) return;
      setLoading(true);
      setError(null);
      try {
        const list = await invoke<SftpEntry[]>('sftp_list_dir_cmd', {
          sessionId,
          remotePath: path,
        });
        setRawEntries(list);
      } catch (e) {
        setError(String(e));
        setRawEntries([]);
      }
      setLoading(false);
    },
    [sessionId, isConnected]
  );

  // ── Effect: reload when path changes, clear on disconnect ──
  useEffect(() => {
    if (!isConnected) {
      setRawEntries([]);
      setLoading(false);
      setError(null);
      setPromptMode(null);
      setPromptTarget(null);
      return;
    }
    loadDirectory(currentPath);
  }, [currentPath, isConnected, loadDirectory]);

  // ── Navigation ──
  const navigateTo = useCallback((path: string) => {
    setCurrentPath(path);
    setPromptMode(null);
    setPromptTarget(null);
  }, []);

  const goUp = useCallback(() => {
    if (currentPath === '/') return;
    const parent = currentPath.replace(/\/[^/]*$/, '') || '/';
    setCurrentPath(parent);
  }, [currentPath]);

  const refresh = useCallback(async () => {
    await loadDirectory(currentPath);
  }, [currentPath, loadDirectory]);

  // ── Upload ──
  // 传输命令已改为非阻塞（后端 spawn 后立即返回 Ok(())）。
  // 目录刷新由 `sftp-transfer-finished` 事件监听器统一处理（见下方 effect）。
  const uploadFile = useCallback(
    async (localPath: string, remotePath: string): Promise<void> => {
      await invoke<void>('sftp_upload_file_cmd', {
        sessionId,
        localPath,
        remotePath,
      });
    },
    [sessionId]
  );

  // ── Download files ──
  // 多文件串行下载：传输命令为非阻塞，需等待 `sftp-transfer-finished` 事件
  // 后再开始下一个文件，避免同一会话并发传输导致取消标志被覆盖。
  const downloadFiles = useCallback(
    async (targetEntries: SftpEntry[]) => {
      const files = targetEntries.filter(e => !e.is_dir);
      for (const entry of files) {
        try {
          const localPath = await save({ defaultPath: entry.name });
          if (!localPath) continue;
          await invoke<void>('sftp_download_file_cmd', {
            sessionId,
            remotePath: entry.path,
            localPath,
          });
          // 等待当前文件传输完成
          await new Promise<void>((resolve, reject) => {
            const unlisten = listen<{ session_id: string; direction: string; file_name: string; result: { error?: string } }>(
              'sftp-transfer-finished',
              (event) => {
                if (event.payload.session_id === sessionId
                    && event.payload.direction === 'download'
                    && event.payload.file_name === entry.name) {
                  unlisten.then(fn => fn());
                  if (event.payload.result.error) {
                    reject(new Error(event.payload.result.error));
                  } else {
                    resolve();
                  }
                }
              }
            );
          });
        } catch (e) {
          setError(`Download failed: ${e}`);
        }
      }
    },
    [sessionId]
  );

  // ── Download directory ──
  const downloadDirectory = useCallback(
    async (remoteDir: string, localDir: string): Promise<void> => {
      await invoke<void>('sftp_download_dir_cmd', {
        sessionId,
        remoteDir,
        localDir,
      });
    },
    [sessionId]
  );

  // ── 传输完成后刷新当前目录（监听后端 `sftp-transfer-finished` 事件）──
  // 上传完成后需要刷新远程目录列表；下载完成后无需刷新但刷新无副作用。
  // 使用 Promise 引用模式避免组件卸载在 .then() 前导致 listener 泄漏。
  useEffect(() => {
    const p = listen<{ session_id: string; direction: string; result: { error?: string } }>(
      'sftp-transfer-finished',
      (event) => {
        if (event.payload.session_id === sessionId && event.payload.direction === 'upload') {
          loadDirectory(currentPath);
        }
      }
    );
    return () => {
      p.then(fn => fn());
    };
  }, [sessionId, currentPath, loadDirectory]);

  // ── Delete entries ──
  const deleteEntries = useCallback(
    async (targetEntries: SftpEntry[]): Promise<string[]> => {
      if (targetEntries.length === 1) {
        const entry = targetEntries[0];
        if (entry.is_dir) {
          await invoke<void>('sftp_delete_recursive_cmd', {
            sessionId,
            remotePath: entry.path,
          });
        } else {
          await invoke<void>('sftp_delete_cmd', {
            sessionId,
            remotePath: entry.path,
          });
        }
        await loadDirectory(currentPath);
        return [];
      }
      // Multi-delete: use recursive for dirs, regular for files
      const failed: string[] = [];
      for (const entry of targetEntries) {
        try {
          if (entry.is_dir) {
            await invoke<void>('sftp_delete_recursive_cmd', {
              sessionId,
              remotePath: entry.path,
            });
          } else {
            await invoke<void>('sftp_delete_cmd', {
              sessionId,
              remotePath: entry.path,
            });
          }
        } catch (e) {
          failed.push(entry.path);
        }
      }
      await loadDirectory(currentPath);
      return failed;
    },
    [sessionId, currentPath, loadDirectory]
  );

  // ── Rename ──
  const renameEntry = useCallback(
    async (entry: SftpEntry, newName: string) => {
      const parentPath = currentPath === '/' ? '' : currentPath;
      const toPath = `${parentPath}/${newName}`;
      await invoke<void>('sftp_rename_cmd', {
        sessionId,
        fromPath: entry.path,
        toPath,
      });
      await loadDirectory(currentPath);
    },
    [sessionId, currentPath, loadDirectory]
  );

  // ── Create file ──
  const createFile = useCallback(
    async (name: string) => {
      const remotePath =
        currentPath === '/'
          ? `/${name}`
          : `${currentPath}/${name}`;
      await invoke<void>('sftp_new_file_cmd', {
        sessionId,
        remotePath,
      });
      await loadDirectory(currentPath);
    },
    [sessionId, currentPath, loadDirectory]
  );

  // ── Create folder ──
  const createFolder = useCallback(
    async (name: string) => {
      const remotePath =
        currentPath === '/'
          ? `/${name}`
          : `${currentPath}/${name}`;
      await invoke<void>('sftp_mkdir_cmd', {
        sessionId,
        remotePath,
      });
      await loadDirectory(currentPath);
    },
    [sessionId, currentPath, loadDirectory]
  );

  // ── Sort toggle ──
  const setSort = useCallback(
    (field: SortField) => {
      setSortField(prevField => {
        if (prevField === field) {
          setSortDirection(prevDir => (prevDir === 'asc' ? 'desc' : 'asc'));
          return prevField;
        }
        setSortDirection('asc');
        return field;
      });
    },
    []
  );

  const clearError = useCallback(() => setError(null), []);

  return useMemo(
    () => ({
      currentPath,
      entries,
      loading,
      error,
      breadcrumbSegments,
      promptMode,
      promptValue,
      promptTarget,
      sortField,
      sortDirection,

      loadDirectory,
      navigateTo,
      goUp,
      refresh,
      uploadFile,
      downloadFiles,
      downloadDirectory,
      deleteEntries,
      renameEntry,
      createFile,
      createFolder,
      setPromptMode,
      setPromptValue,
      setPromptTarget,
      setSortField: setSort,
      clearError,
    }),
    [
      currentPath,
      entries,
      loading,
      error,
      breadcrumbSegments,
      promptMode,
      promptValue,
      promptTarget,
      sortField,
      sortDirection,
      loadDirectory,
      navigateTo,
      goUp,
      refresh,
      uploadFile,
      downloadFiles,
      downloadDirectory,
      deleteEntries,
      renameEntry,
      createFile,
      createFolder,
      setPromptMode,
      setPromptValue,
      setPromptTarget,
      setSort,
      clearError,
    ]
  );
}
