import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { useSession } from '../../../context/SessionContext';
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
  type SftpAvailability,
  type SftpListing,
  type TransferStartAck,
  type OverwritePolicy,
} from '../types';
import { sftpErrorCode, sftpErrorMessage } from '../../../services/sftpErrors';
import { isDirectory, isRegularFile, isSymlink } from '../entryKind';

export interface UseFileManagerReturn {
  currentPath: string | null;
  entries: SftpEntry[];
  loading: boolean;
  error: string | null;
  /** 后端 SFTP 能力探测结果；null 表示正在探测。 */
  sftpAvailability: SftpAvailability | null;
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
  /** 重新探测 SFTP 能力并从后端解析的起始目录重新打开（不可用态的“重试”）。 */
  retrySftp: () => void;
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
  // 排序分组只把真实目录（entry_type 为 directory）放在前面；符号链接即使指向
  // 目录也留在文件组，避免“看起来是目录却被当成目录排到上面”的歧义。
  const dirs = list.filter(entry => isDirectory(entry));
  const files = list.filter(entry => !isDirectory(entry));

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
  const { state: sessions } = useSession();
  const runtimeId = sessions.tabs.find(tab => tab.id === sessionId)?.runtimeId;
  const requireRuntimeId = useCallback(() => {
    if (!runtimeId) throw new Error('Session runtime is not connected');
    return runtimeId;
  }, [runtimeId]);
  const transferTask = getTaskForSession(sessionId);
  const [currentPath, setCurrentPath] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [sftpAvailability, setSftpAvailability] = useState<SftpAvailability | null>(null);
  const [rawEntries, setRawEntries] = useState<SftpEntry[]>([]);
  const [sortField, setSortField] = useState<SortField>('name');
  const [sortDirection, setSortDirection] = useState<SortDirection>('asc');
  const [promptMode, setPromptMode] = useState<PromptMode | null>(null);
  const [promptValue, setPromptValue] = useState('');
  const [promptTarget, setPromptTarget] = useState<SftpEntry | null>(null);
  const directoryGenerationRef = useRef(0);
  const availabilityGenerationRef = useRef(0);
  const loadedPathRef = useRef<string | null>(null);
  const currentPathRef = useRef(currentPath);
  const refreshedTransferIdRef = useRef<string | null>(null);
  currentPathRef.current = currentPath;

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

  // 能力探测：后端缓存确定性结果（可用 / 服务端不支持子系统），`refresh` 会丢弃
  // 后端缓存重新探测；前端只缓存展示结果。
  const probeSftpAvailability = useCallback(
    async (targetSessionId: string, refresh = false): Promise<boolean> => {
      const generation = ++availabilityGenerationRef.current;
      setLoading(true);
      try {
        const availability = await invoke<SftpAvailability>('ssh_sftp_availability_cmd', {
          sessionId: targetSessionId,
          refresh,
        });
        if (generation !== availabilityGenerationRef.current) return false;
        setSftpAvailability(availability);
        if (!availability.available) {
          setRawEntries([]);
          setLoading(false);
        }
        return availability.available;
      } catch (probeError) {
        if (generation !== availabilityGenerationRef.current) return false;
        setSftpAvailability({ available: false, reason: sftpErrorMessage(probeError) });
        setRawEntries([]);
        setLoading(false);
        return false;
      }
    },
    [],
  );

  // 打开目录：`remotePath` 省略时由后端通过 SFTP realpath 解析起始目录；
  // 返回的 `path` 是规范化后的当前目录，前端必须采用它，不得猜测（例如回退到 "/"）。
  const loadListing = useCallback(
    async (remotePath?: string): Promise<boolean> => {
      if (!isConnected) return false;
      const generation = ++directoryGenerationRef.current;
      setLoading(true);
      setError(null);
      try {
        const listing = await invoke<SftpListing>(
          'sftp_list_dir_cmd',
          remotePath ? { sessionId, remotePath } : { sessionId },
        );
        if (generation !== directoryGenerationRef.current) return false;
        loadedPathRef.current = listing.path;
        setRawEntries(listing.entries);
        setCurrentPath(listing.path);
        return true;
      } catch (requestError) {
        if (generation !== directoryGenerationRef.current) return false;
        const code = sftpErrorCode(requestError);
        setError(sftpErrorMessage(requestError));
        // unsupported 与 available=false 等价：服务端不提供 sftp 子系统。
        if (code === 'unsupported') {
          setSftpAvailability({ available: false, reason: sftpErrorMessage(requestError) });
          loadedPathRef.current = null;
          setCurrentPath(null);
          setRawEntries([]);
          // 原因已经进入不可用态，避免错误横幅重复展示同一段文本。
          setError(null);
          return false;
        }
        // 导航目标已在远端消失：回到最后一次成功打开的目录并保留错误提示，
        // 避免面板停留在已不存在的路径上。
        if (code === 'not_found' && remotePath !== undefined) {
          const lastGoodPath = loadedPathRef.current;
          if (lastGoodPath !== null && lastGoodPath !== remotePath) {
            setCurrentPath(lastGoodPath);
            return false;
          }
        }
        // 列表清空后必须让当前路径重新可加载：否则导航回上一次成功目录时，
        // `loadedPathRef === currentPath` 会让 effect 提前返回，出现空列表 + 旧错误横幅。
        loadedPathRef.current = null;
        setRawEntries([]);
        return false;
      } finally {
        if (generation === directoryGenerationRef.current) {
          setLoading(false);
        }
      }
    },
    [isConnected, sessionId],
  );

  const loadDirectory = useCallback(
    async (path: string) => {
      await loadListing(path);
    },
    [loadListing],
  );

  // 连接/会话变化时清空状态；连接建立后先探测能力，再以“无路径”方式打开起始目录。
  useEffect(() => {
    availabilityGenerationRef.current += 1;
    directoryGenerationRef.current += 1;
    refreshedTransferIdRef.current = null;
    loadedPathRef.current = null;
    setCurrentPath(null);
    setRawEntries([]);
    setLoading(false);
    setError(null);
    setSftpAvailability(null);
    setPromptMode(null);
    setPromptTarget(null);
    if (!isConnected) return;
    void probeSftpAvailability(sessionId);
  }, [isConnected, probeSftpAvailability, sessionId]);

  // 能力可用后才加载目录：起始目录由后端解析；导航时传入目标路径并采用返回的规范化路径。
  useEffect(() => {
    if (!isConnected || !sftpAvailability?.available) return;
    if (currentPath === null) {
      void loadListing();
      return;
    }
    if (loadedPathRef.current === currentPath) return;
    void loadListing(currentPath);
  }, [currentPath, isConnected, loadListing, sftpAvailability]);

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
    if (!isConnected || !sftpAvailability?.available) return;
    await loadListing(currentPath ?? undefined);
  }, [currentPath, isConnected, loadListing, sftpAvailability]);

  // 不可用态的“重试”：强制后端重新探测能力（丢弃缓存），再从起始目录重新打开。
  const retrySftp = useCallback(() => {
    availabilityGenerationRef.current += 1;
    directoryGenerationRef.current += 1;
    loadedPathRef.current = null;
    setCurrentPath(null);
    setRawEntries([]);
    setError(null);
    setSftpAvailability(null);
    if (!isConnected) return;
    void probeSftpAvailability(sessionId, true);
  }, [isConnected, probeSftpAvailability, sessionId]);

  // 文件管理器只表达“启动 SFTP”的用户意图；命令名、ack 和等待竞态集中在 TransferService。
  const uploadFiles = useCallback(
    async (
      localPaths: string[],
      remoteDir: string,
      overwritePolicy: OverwritePolicy = 'keep-both',
    ): Promise<TransferStartAck> => {
      return startFileTransfer('send', {
        sessionId,
        runtimeId: requireRuntimeId(),
        protocolOptions: { protocol: 'sftp' },
        filePaths: localPaths,
        remoteDir,
        overwritePolicy,
      });
    },
    [sessionId, requireRuntimeId],
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
      // 调用方已把链接目标分类成文件流程；普通文件与文件目标链接都走单文件下载。
      const files = targetEntries.filter(entry => isRegularFile(entry) || isSymlink(entry));
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
            runtimeId: requireRuntimeId(),
            protocolOptions: { protocol: 'sftp' },
            downloadDir,
            remotePaths,
            destinationPaths,
            overwritePolicy: files.length === 1 ? 'replace' : 'keep-both',
          },
          sessionId,
          'sftp',
        );
      } catch (downloadError) {
        setError(`Download failed: ${sftpErrorMessage(downloadError)}`);
      }
    },
    [sessionId, requireRuntimeId],
  );

  const downloadDirectory = useCallback(
    async (remoteDir: string, localDir: string): Promise<void> => {
      try {
        await startFileTransferAndWait(
          'receive',
          {
            sessionId,
            runtimeId: requireRuntimeId(),
            protocolOptions: { protocol: 'sftp' },
            downloadDir: localDir,
            remotePaths: [remoteDir],
            overwritePolicy: 'keep-both',
          },
          sessionId,
          'sftp',
        );
      } catch (downloadError) {
        setError(`Download failed: ${sftpErrorMessage(downloadError)}`);
        throw downloadError;
      }
    },
    [sessionId, requireRuntimeId],
  );

  // 默认策略仍为每 Session 单活动任务，因此目录批量下载按目录顺序执行。
  const downloadDirectories = useCallback(
    async (dirEntries: SftpEntry[], localRootDir: string): Promise<string[]> => {
      const failed: string[] = [];

      for (const entry of dirEntries) {
        // 调用方已把链接目标分类成目录流程；真实目录与目录目标链接都以逻辑路径
        // 交给后端，后端对顶层链接只跟随一次。
        if (!isDirectory(entry) && !isSymlink(entry)) continue;
        try {
          await startFileTransferAndWait(
            'receive',
            {
              sessionId,
              runtimeId: requireRuntimeId(),
              protocolOptions: { protocol: 'sftp' },
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
    [sessionId, requireRuntimeId],
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
    if (!path || !isConnected || !sftpAvailability?.available) return;

    const generation = ++directoryGenerationRef.current;
    invoke<SftpListing>('sftp_list_dir_cmd', {
      sessionId,
      remotePath: path,
    }).then(listing => {
      if (
        generation === directoryGenerationRef.current
        && currentPathRef.current === path
      ) {
        setRawEntries(listing.entries);
      }
    }).catch(refreshError => {
      if (
        generation === directoryGenerationRef.current
        && currentPathRef.current === path
      ) {
        setError(sftpErrorMessage(refreshError));
      }
    });
  }, [
    isConnected,
    sessionId,
    sftpAvailability,
    transferTask?.phase,
    transferTask?.protocol,
    transferTask?.transferId,
  ]);

  const deleteEntries = useCallback(
    async (targetEntries: SftpEntry[]): Promise<string[]> => {
      if (currentPath === null) return [];
      if (targetEntries.length === 1) {
        const entry = targetEntries[0];
        // 递归删除只用于真实目录；符号链接只删除链接本身，绝不递归进目标。
        if (isDirectory(entry)) {
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
          if (isDirectory(entry)) {
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
      sftpAvailability,
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
      retrySftp,
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
      retrySftp,
      setSort,
      sortDirection,
      sortField,
      sftpAvailability,
      uploadFile,
      uploadFiles,
    ],
  );
}
