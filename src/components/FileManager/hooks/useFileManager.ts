import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { save, open } from '@tauri-apps/plugin-dialog';
import {
  isManagedTransferTerminalPhase,
  useTransfer,
} from '../../../context/TransferContext';
import {
  startFileTransfer,
  startFileTransferAndWait,
} from '../../../services/transferService';
import {
  SftpEntry,
  PromptMode,
  SortField,
  SortDirection,
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
  direction: SortDirection,
): SftpEntry[] {
  const dirs = list.filter(entry => entry.is_dir);
  const files = list.filter(entry => !entry.is_dir);

  const compare = (a: SftpEntry, b: SftpEntry): number => {
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

  dirs.sort(compare);
  files.sort(compare);
  return [...dirs, ...files];
}

export function useFileManager(
  sessionId: string,
  isConnected: boolean,
): UseFileManagerReturn {
  const { getTaskForSession } = useTransfer();
  const transferTask = getTaskForSession(sessionId);
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
  const currentPathRef = useRef(currentPath);
  const refreshedTransferIdRef = useRef<string | null>(null);
  currentPathRef.current = currentPath;

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

  const entries = useMemo(
    () => sortEntries(rawEntries, sortField, sortDirection),
    [rawEntries, sortField, sortDirection],
  );

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
      ...segments.map((segment, index) => ({
        name: segment,
        path: '/' + segments.slice(0, index + 1).join('/'),
      })),
    ];
  }, [currentPath]);

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
      } catch (requestError) {
        if (generation !== directoryGenerationRef.current) return;
        setError(String(requestError));
        setRawEntries([]);
      } finally {
        if (generation === directoryGenerationRef.current) {
          setLoading(false);
        }
      }
    },
    [isConnected, sessionId],
  );

  useEffect(() => {
    if (!isConnected) {
      directoryGenerationRef.current += 1;
      refreshedTransferIdRef.current = null;
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

  // 文件管理器只表达“启动 SFTP”的用户意图；命令名、ack 和等待竞态集中在 TransferService。
  const uploadFiles = useCallback(
    async (
      localPaths: string[],
      remoteDir: string,
      overwritePolicy: OverwritePolicy = 'keep-both',
    ): Promise<TransferStartAck> => {
      return startFileTransfer('send', {
        sessionId,
        protocol: 'sftp',
        filePaths: localPaths,
        remoteDir,
        overwritePolicy,
      });
    },
    [sessionId],
  );

  const uploadFile = useCallback(
    async (localPath: string, remotePath: string): Promise<TransferStartAck> => {
      const remoteDir = remotePath.substring(0, remotePath.lastIndexOf('/') + 1 || 0) || '/';
      return uploadFiles([localPath], remoteDir);
    },
    [uploadFiles],
  );

  const downloadFiles = useCallback(
    async (targetEntries: SftpEntry[]) => {
      const files = targetEntries.filter(entry => !entry.is_dir);
      if (files.length === 0) return;

      let downloadDir: string;
      let remotePaths: string[];
      let destinationPaths: string[] | undefined;

      if (files.length === 1) {
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
        const dir = await open({ directory: true, multiple: false });
        if (!dir) return;
        downloadDir = typeof dir === 'string' ? dir : (dir as string);
        remotePaths = files.map(file => file.path);
        destinationPaths = undefined;
      }

      try {
        await startFileTransferAndWait(
          'receive',
          {
            sessionId,
            protocol: 'sftp',
            downloadDir,
            remotePaths,
            destinationPaths,
            overwritePolicy: files.length === 1 ? 'replace' : 'keep-both',
          },
          sessionId,
          'sftp',
        );
      } catch (downloadError) {
        setError(`Download failed: ${downloadError}`);
      }
    },
    [sessionId],
  );

  const downloadDirectory = useCallback(
    async (remoteDir: string, localDir: string): Promise<void> => {
      try {
        await startFileTransferAndWait(
          'receive',
          {
            sessionId,
            protocol: 'sftp',
            downloadDir: localDir,
            remotePaths: [remoteDir],
            overwritePolicy: 'keep-both',
          },
          sessionId,
          'sftp',
        );
      } catch (downloadError) {
        setError(`Download failed: ${downloadError}`);
        throw downloadError;
      }
    },
    [sessionId],
  );

  // 默认策略仍为每 Session 单活动任务，因此目录批量下载按目录顺序执行。
  const downloadDirectories = useCallback(
    async (dirEntries: SftpEntry[], localRootDir: string): Promise<string[]> => {
      const failed: string[] = [];

      for (const entry of dirEntries) {
        if (!entry.is_dir) continue;
        try {
          await startFileTransferAndWait(
            'receive',
            {
              sessionId,
              protocol: 'sftp',
              downloadDir: localRootDir,
              remotePaths: [entry.path],
              overwritePolicy: 'keep-both',
            },
            sessionId,
            'sftp',
          );
        } catch (downloadError) {
          if (import.meta.env.DEV) {
            console.error(`Download directory "${entry.name}" failed:`, downloadError);
          }
          failed.push(entry.name);
        }
      }

      return failed;
    },
    [sessionId],
  );

  // 传输终态来自统一 TransferContext。文件管理器不再单独监听 backend finished 事件；
  // 成功、部分失败或取消都可能改变远端目录，因此每个精确 transfer_id 只静默刷新一次。
  useEffect(() => {
    if (
      !transferTask
      || transferTask.protocol !== 'sftp'
      || !isManagedTransferTerminalPhase(transferTask.phase)
      || refreshedTransferIdRef.current === transferTask.transferId
    ) {
      return;
    }
    refreshedTransferIdRef.current = transferTask.transferId;
    const path = currentPathRef.current;
    if (!path || !isConnected) return;

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
    }).catch(refreshError => {
      if (
        generation === directoryGenerationRef.current
        && currentPathRef.current === path
      ) {
        setError(String(refreshError));
      }
    });
  }, [
    isConnected,
    sessionId,
    transferTask?.phase,
    transferTask?.protocol,
    transferTask?.transferId,
  ]);

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
        } catch {
          failed.push(entry.path);
        }
      }
      await loadDirectory(currentPath);
      return failed;
    },
    [currentPath, loadDirectory, sessionId],
  );

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
    [currentPath, loadDirectory, sessionId],
  );

  const createFile = useCallback(
    async (name: string) => {
      if (currentPath === null) return;
      const remotePath = currentPath === '/' ? `/${name}` : `${currentPath}/${name}`;
      await invoke<void>('sftp_new_file_cmd', {
        sessionId,
        remotePath,
      });
      await loadDirectory(currentPath);
    },
    [currentPath, loadDirectory, sessionId],
  );

  const createFolder = useCallback(
    async (name: string) => {
      if (currentPath === null) return;
      const remotePath = currentPath === '/' ? `/${name}` : `${currentPath}/${name}`;
      await invoke<void>('sftp_mkdir_cmd', {
        sessionId,
        remotePath,
      });
      await loadDirectory(currentPath);
    },
    [currentPath, loadDirectory, sessionId],
  );

  const setSort = useCallback((field: SortField) => {
    setSortField(previousField => {
      if (previousField === field) {
        setSortDirection(previousDirection => previousDirection === 'asc' ? 'desc' : 'asc');
        return previousField;
      }
      setSortDirection('asc');
      return field;
    });
  }, []);

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
      breadcrumbSegments,
      clearError,
      createFile,
      createFolder,
      currentPath,
      deleteEntries,
      downloadDirectories,
      downloadDirectory,
      downloadFiles,
      entries,
      error,
      goUp,
      loadDirectory,
      loading,
      navigateTo,
      promptMode,
      promptTarget,
      promptValue,
      refresh,
      renameEntry,
      setSort,
      sortDirection,
      sortField,
      uploadFile,
      uploadFiles,
    ],
  );
}
