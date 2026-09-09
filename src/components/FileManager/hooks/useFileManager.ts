import { useState, useCallback, useEffect, useMemo, useRef } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { save, open } from '@tauri-apps/plugin-dialog';
import {
  SftpEntry,
  PromptMode,
  SortField,
  SortDirection,
  type TransferFinishedPayload,
  type TransferStartAck,
  type OverwritePolicy,
} from '../types';

export interface UseFileManagerReturn {
  currentPath: string | null;
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
  uploadFile: (localPath: string, remotePath: string) => Promise<TransferStartAck>;
  uploadFiles: (
    localPaths: string[],
    remoteDir: string,
    overwritePolicy?: OverwritePolicy,
  ) => Promise<TransferStartAck>;
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


async function runSftpTransferAndWait(
  sessionId: string,
  startTransfer: () => Promise<TransferStartAck>,
): Promise<void> {
  let activeTransferId: string | null = null;
  const bufferedFinished = new Map<string, TransferFinishedPayload>();
  let unlistenFinished: (() => void) | undefined;

  let resolveFinished!: () => void;
  let rejectFinished!: (error: Error) => void;
  let settled = false;
  const finishedPromise = new Promise<void>((resolve, reject) => {
    resolveFinished = resolve;
    rejectFinished = reject;
  });
  const settle = (payload: TransferFinishedPayload) => {
    if (settled) return;
    settled = true;
    if (payload.success) resolveFinished();
    else rejectFinished(new Error(payload.error || 'SFTP transfer failed'));
  };

  try {
    // 先注册 finished，避免极小文件在 invoke 返回 ack 前已经完成。
    unlistenFinished = await listen<TransferFinishedPayload>(
      'file-transfer:finished',
      (event) => {
        const payload = event.payload;
        if (payload.session_id !== sessionId) return;
        if (payload.protocol && payload.protocol !== 'sftp') return;
        if (!activeTransferId) {
          if (payload.transfer_id) {
            bufferedFinished.set(payload.transfer_id, payload);
          }
          return;
        }
        if (payload.transfer_id !== activeTransferId) return;
        settle(payload);
      },
    );

    const ack = await startTransfer();
    activeTransferId = ack.transfer_id;
    const earlyFinished = bufferedFinished.get(activeTransferId);
    if (earlyFinished) {
      settle(earlyFinished);
    }
    await finishedPromise;
  } finally {
    unlistenFinished?.();
  }
}

export function useFileManager(
  sessionId: string,
  isConnected: boolean
): UseFileManagerReturn {
  const [currentPath, setCurrentPath] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [rawEntries, setRawEntries] = useState<SftpEntry[]>([]);
  const [sortField, setSortField] = useState<SortField>('name');
  const [sortDirection, setSortDirection] = useState<SortDirection>('asc');
  const [promptMode, setPromptMode] = useState<PromptMode | null>(null);
  const [promptValue, setPromptValue] = useState('');
  const [promptTarget, setPromptTarget] = useState<SftpEntry | null>(null);
  const directoryGenerationRef = useRef(0);

  // ── Resolve remote home dir on connection ──
  const connectedRef = useRef(isConnected);
  connectedRef.current = isConnected;

  useEffect(() => {
    if (!isConnected) return;
    invoke<string>('get_ssh_home_dir', { sessionId })
      .then(homeDir => {
        if (connectedRef.current) setCurrentPath(homeDir);
      })
      .catch(() => {
        if (connectedRef.current) setCurrentPath('/');
      });
  }, [isConnected, sessionId]);

  // ── Sorted entries (dirs first, then by sortField/direction) ──
  const entries = useMemo(
    () => sortEntries(rawEntries, sortField, sortDirection),
    [rawEntries, sortField, sortDirection]
  );

  // ── Breadcrumb segments ──
  const breadcrumbSegments = useMemo((): { name: string; path: string }[] => {
    if (currentPath === null) {
      return [{ name: '...', path: '/' }];
    }
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
      const generation = ++directoryGenerationRef.current;
      setLoading(true);
      setError(null);
      try {
        const list = await invoke<SftpEntry[]>('sftp_list_dir_cmd', {
          sessionId,
          remotePath: path,
        });
        if (generation !== directoryGenerationRef.current) return;
        setRawEntries(list);
      } catch (e) {
        if (generation !== directoryGenerationRef.current) return;
        setError(String(e));
        setRawEntries([]);
      } finally {
        if (generation === directoryGenerationRef.current) {
          setLoading(false);
        }
      }
    },
    [sessionId, isConnected]
  );

  // ── Effect: reload when path changes, clear on disconnect ──
  useEffect(() => {
    if (!isConnected) {
      directoryGenerationRef.current += 1;
      setCurrentPath(null);
      setRawEntries([]);
      setLoading(false);
      setError(null);
      setPromptMode(null);
      setPromptTarget(null);
      return;
    }
    if (currentPath === null) {
      setLoading(true);
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
    if (currentPath === null || currentPath === '/') return;
    const parent = currentPath.replace(/\/[^/]*$/, '') || '/';
    setCurrentPath(parent);
  }, [currentPath]);

  const refresh = useCallback(async () => {
    if (currentPath === null) return;
    await loadDirectory(currentPath);
  }, [currentPath, loadDirectory]);

  // ── Upload ──
  // 使用统一传输命令（非阻塞，后端 spawn 后立即返回）。
  // 进度由 `file-transfer:progress` 事件统一处理（见 useSftpProgress）。
  const uploadFiles = useCallback(
    async (
      localPaths: string[],
      remoteDir: string,
      overwritePolicy: OverwritePolicy = 'keep-both',
    ): Promise<TransferStartAck> => {
      return invoke<TransferStartAck>('file_transfer_send', {
        request: {
          sessionId,
          protocol: 'sftp',
          filePaths: localPaths,
          remoteDir,
          overwritePolicy,
        },
      });
    },
    [sessionId]
  );

  const uploadFile = useCallback(
    async (localPath: string, remotePath: string): Promise<TransferStartAck> => {
      const remoteDir = remotePath.substring(0, remotePath.lastIndexOf('/') + 1 || 0) || '/';
      return uploadFiles([localPath], remoteDir);
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
      let destinationPaths: string[] | undefined;

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
        destinationPaths = [localPath as string];
      } else {
        // 多文件：一次目录选择器，全部文件批量下载
        const dir = await open({ directory: true, multiple: false });
        if (!dir) return;
        downloadDir = typeof dir === 'string' ? dir : (dir as string);
        remotePaths = files.map(f => f.path);
        destinationPaths = undefined;
      }

      try {
        await runSftpTransferAndWait(sessionId, () =>
          invoke<TransferStartAck>('file_transfer_receive', {
            request: {
              sessionId,
              protocol: 'sftp',
              downloadDir,
              remotePaths,
              destinationPaths,
              overwritePolicy: files.length === 1 ? 'replace' : 'keep-both',
            },
          }),
        );
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
      try {
        await runSftpTransferAndWait(sessionId, () =>
          invoke<TransferStartAck>('file_transfer_receive', {
            request: {
              sessionId,
              protocol: 'sftp',
              downloadDir: localDir,
              remotePaths: [remoteDir],
              overwritePolicy: 'keep-both',
            },
          }),
        );
      } catch (error) {
        setError(`Download failed: ${error}`);
        throw error;
      }
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

        try {
          await runSftpTransferAndWait(sessionId, () =>
            invoke<TransferStartAck>('file_transfer_receive', {
              request: {
                sessionId,
                protocol: 'sftp',
                downloadDir: localRootDir,
                remotePaths: [entry.path],
                overwritePolicy: 'keep-both',
              },
            }),
          );
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

  // ── SFTP 传输结束后刷新当前目录（成功/部分失败/取消都可能改变远端目录）──
  // 使用 ref 保持最新 currentPath，避免监听器因目录导航反复注册/注销
  const currentPathRef = useRef(currentPath);
  currentPathRef.current = currentPath;

  useEffect(() => {
    const p = listen<TransferFinishedPayload>(
      'file-transfer:finished',
      (event) => {
        const path = currentPathRef.current;
        if (
          path !== null
          && event.payload.session_id === sessionId
          && (!event.payload.protocol || event.payload.protocol === 'sftp')
        ) {
          // 静默刷新目录（不触发 loading 闪烁），仅更新条目列表
          const generation = ++directoryGenerationRef.current;
          invoke<SftpEntry[]>('sftp_list_dir_cmd', {
            sessionId,
            remotePath: path,
          }).then(list => {
            if (
              generation === directoryGenerationRef.current
              && currentPathRef.current === path
            ) {
              setRawEntries(list);
            }
          }).catch(e => {
            if (
              generation === directoryGenerationRef.current
              && currentPathRef.current === path
            ) {
              setError(String(e));
            }
          });
        }
      }
    );
    return () => {
      p.then(fn => fn());
    };
  }, [sessionId]);

  // ── Delete entries ──
  const deleteEntries = useCallback(
    async (targetEntries: SftpEntry[]): Promise<string[]> => {
      if (currentPath === null) return [];
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
      if (currentPath === null) return;
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
      if (currentPath === null) return;
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
      if (currentPath === null) return;
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
      downloadDirectories,
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
