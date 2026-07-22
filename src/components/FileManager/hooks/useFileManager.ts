import { useState, useCallback, useEffect, useMemo } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { save, open } from '@tauri-apps/plugin-dialog';
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
  uploadFiles: (localPaths: string[], remoteDir: string) => Promise<void>;
  downloadFiles: (entries: SftpEntry[]) => Promise<void>;
  downloadDirectory: (remoteDir: string, localDir: string) => Promise<void>;
  downloadDirectories: (dirEntries: SftpEntry[], localRootDir: string) => Promise<string[]>;
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
      setCurrentPath('/');
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
  // 使用统一传输命令（非阻塞，后端 spawn 后立即返回）。
  // 进度由 `file-transfer:progress` 事件统一处理（见 useSftpProgress）。
  const uploadFiles = useCallback(
    async (localPaths: string[], remoteDir: string): Promise<void> => {
      await invoke<void>('file_transfer_send', {
        sessionId,
        protocol: 'sftp',
        filePaths: localPaths,
        remoteDir,
      });
    },
    [sessionId]
  );

  const uploadFile = useCallback(
    async (localPath: string, remotePath: string): Promise<void> => {
      const remoteDir = remotePath.substring(0, remotePath.lastIndexOf('/') + 1 || 0) || '/';
      await uploadFiles([localPath], remoteDir);
    },
    [uploadFiles]
  );

  // ── Download files ──
  // 单文件：save 对话框（可重命名）；多文件：目录选择器 + 一次性批量下载
  const downloadFiles = useCallback(
    async (targetEntries: SftpEntry[]) => {
      const files = targetEntries.filter(e => !e.is_dir);
      if (files.length === 0) return;

      // 确定下载目标目录和远程路径列表
      let downloadDir: string;
      let remotePaths: string[];

      if (files.length === 1) {
        // 单文件：save 对话框（可重命名），提取父目录
        const localPath = await save({ defaultPath: files[0].name });
        if (!localPath) return;
        const lastSep = Math.max(
          (localPath as string).lastIndexOf('/'),
          (localPath as string).lastIndexOf('\\'),
        );
        downloadDir = lastSep >= 0 ? (localPath as string).substring(0, lastSep) : '.';
        remotePaths = [files[0].path];
      } else {
        // 多文件：一次目录选择器，全部文件批量下载
        const dir = await open({ directory: true, multiple: false });
        if (!dir) return;
        downloadDir = typeof dir === 'string' ? dir : (dir as string);
        remotePaths = files.map(f => f.path);
      }

      try {
        // 注册监听器在 invoke 之前，避免竞态（后端 SideChannel 路径立即返回）
        const TRANSFER_TIMEOUT_MS = 5 * 60 * 1000;
        let unlistenFn: (() => void) | undefined;
        const finishedPromise = new Promise<void>((resolve, reject) => {
          const timeoutId = setTimeout(() => {
            unlistenFn?.();
            reject(new Error('Download timed out after 5 minutes'));
          }, TRANSFER_TIMEOUT_MS);

          listen<{ session_id: string; success: boolean; error?: string }>(
            'file-transfer:finished',
            (event) => {
              if (event.payload.session_id === sessionId) {
                clearTimeout(timeoutId);
                unlistenFn?.();
                if (event.payload.error) {
                  reject(new Error(event.payload.error));
                } else {
                  resolve();
                }
              }
            }
          ).then(fn => { unlistenFn = fn; }).catch(reject);
        });

        await invoke<void>('file_transfer_receive', {
          sessionId,
          protocol: 'sftp',
          downloadDir,
          remotePaths,
        });

        await finishedPromise;
      } catch (e) {
        setError(`Download failed: ${e}`);
      }
    },
    [sessionId]
  );

  // ── Download directory ──
  // 后端 SftpFileTransfer::receive() 检测到目录路径后自动递归列举子文件
  const downloadDirectory = useCallback(
    async (remoteDir: string, localDir: string): Promise<void> => {
      const dirName = remoteDir.split('/').pop() || 'download';
      await invoke<void>('file_transfer_receive', {
        sessionId,
        protocol: 'sftp',
        downloadDir: `${localDir}/${dirName}`,
        remotePaths: [remoteDir],
      });
    },
    [sessionId]
  );

  // ── Download multiple directories sequentially ──
  // 每个远程目录下载到 localRootDir/dirName/ 子文件夹中。
  // 由于 session 级别只允许一个传输运行，采用顺序 await 模式。
  // 在 invoke 前用 async executor 注册监听器，避免竞态。
  const downloadDirectories = useCallback(
    async (dirEntries: SftpEntry[], localRootDir: string): Promise<string[]> => {
      const failed: string[] = [];

      for (const entry of dirEntries) {
        if (!entry.is_dir) continue;

        const dirName = entry.path.split('/').pop() || 'download';

        // 注册一次性完成监听器（invoke 前设置，避免竞态）
        const TRANSFER_TIMEOUT_MS = 5 * 60 * 1000;
        let unlistenFn: (() => void) | undefined;
        const finishedPromise = new Promise<'ok' | { error: string }>(
          (resolve, reject) => {
            let done = false;
            const timeoutId = setTimeout(() => {
              unlistenFn?.();
              reject(new Error('Download timed out after 5 minutes'));
            }, TRANSFER_TIMEOUT_MS);

            listen<{
              session_id: string;
              success: boolean;
              error?: string;
            }>('file-transfer:finished', (event) => {
              if (event.payload.session_id !== sessionId || done) return;
              done = true;
              clearTimeout(timeoutId);
              unlistenFn?.();
              resolve(
                event.payload.success
                  ? ('ok' as const)
                  : { error: event.payload.error || '传输失败' },
              );
            }).then(fn => { unlistenFn = fn; }).catch(reject);
          },
        );

        try {
          await invoke<void>('file_transfer_receive', {
            sessionId,
            protocol: 'sftp',
            downloadDir: `${localRootDir}/${dirName}`,
            remotePaths: [entry.path],
          });

          const result = await finishedPromise;
          if (result !== 'ok') {
            if (import.meta.env.DEV)
              console.error(`Download directory "${entry.name}" failed:`, result.error);
            failed.push(entry.name);
          }
        } catch (e) {
          if (import.meta.env.DEV)
            console.error(`Download directory "${entry.name}" failed:`, e);
          failed.push(entry.name);
        }
      }

      return failed;
    },
    [sessionId],
  );

  // ── 传输完成后刷新当前目录（监听统一 `file-transfer:finished` 事件）──
  useEffect(() => {
    const p = listen<{ session_id: string; success: boolean }>(
      'file-transfer:finished',
      (event) => {
        if (event.payload.session_id === sessionId && event.payload.success) {
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
      uploadFiles,
      downloadFiles,
      downloadDirectory,
      downloadDirectories,
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
      uploadFiles,
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
