import {
  createContext,
  useContext,
  useReducer,
  useCallback,
  useEffect,
  useRef,
  type ReactNode,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  TransferDirection,
  TransferStatus,
  TransferProgress,
  TransferHistoryItem,
  BatchFileEntry,
  BatchFileResult,
  FileTransferState,
  FileStartEvent,
  FileCompleteEvent,
  TransferCompleteEvent,
  TransferConfig,
  ProtocolType,
  YmodemTransferConfig,
} from "../types/transfer";
import { PROTOCOL_REGISTRY } from "../types/transfer";

export type {
  TransferDirection,
  TransferStatus,
  TransferProgress,
  TransferHistoryItem,
  BatchFileEntry,
};

// ── Command Routing Table ─────────────────────────────────

/** 协议 → 方向 → Tauri 命令名（统一使用 send_files / receive_files） */
const COMMAND_MAP: Record<
  ProtocolType,
  Record<TransferDirection, string>
> = {
  ymodem: {
    send: "send_files",
    receive: "receive_files",
  },
  xmodem: {
    send: "send_files",
    receive: "receive_files",
  },
  zmodem: {
    send: "send_files",
    receive: "receive_files",
  },
  sftp: {
    send: "sftp_upload_file_cmd",
    receive: "sftp_download_file_cmd",
  },
};

// ── State ─────────────────────────────────────────────────

interface TransferState {
  status: TransferStatus;
  progress: TransferProgress | null;
  history: TransferHistoryItem[];
  error: string | null;
  /** 批次文件追踪 Map<fileName, BatchFileEntry> */
  batchFiles: Record<string, BatchFileEntry>;
  /** 批次聚合进度 */
  aggregateBytesTransferred: number;
  aggregateTotalBytes: number;
  currentFileIndex: number;
  totalFiles: number;
  /** 当前活跃传输使用的协议 */
  activeProtocol: ProtocolType | null;
  /** 当前传输速度（bytes/s） */
  speed: number;
  /** 传输开始时间戳（ms），用于速度计算 */
  transferStartTime: number;
}

type TransferAction =
  | { type: "SET_STATUS"; status: TransferStatus }
  | { type: "SET_PROGRESS"; progress: TransferProgress }
  | { type: "ADD_HISTORY"; item: TransferHistoryItem }
  | { type: "CLEAR_HISTORY" }
  | { type: "SET_ERROR"; error: string | null }
  | { type: "INIT_BATCH"; fileNames: string[] }
  | { type: "FILE_START"; event: FileStartEvent }
  | { type: "FILE_COMPLETE"; event: FileCompleteEvent }
  | { type: "SYNC_BATCH_RESULTS"; results: BatchFileResult[] }
  | { type: "RESET_BATCH" }
  | { type: "SET_ACTIVE_PROTOCOL"; protocol: ProtocolType | null };

const initialState: TransferState = {
  status: "idle",
  progress: null,
  history: [],
  error: null,
  batchFiles: {},
  aggregateBytesTransferred: 0,
  aggregateTotalBytes: 0,
  currentFileIndex: 0,
  totalFiles: 0,
  activeProtocol: null,
  speed: 0,
  transferStartTime: 0,
};

function transferReducer(
  state: TransferState,
  action: TransferAction,
): TransferState {
  switch (action.type) {
    case "SET_STATUS": {
      return {
        ...state,
        status: action.status,
      };
    }
    case "SET_PROGRESS": {
      const p = action.progress;
      const key = p.file_name;
      const aggregateBytes =
        p.aggregate_bytes_transferred ?? p.bytes_transferred;
      // 接收端无 INIT_BATCH，首次 progress 事件时初始化计时起点
      const startTime =
        state.transferStartTime > 0
          ? state.transferStartTime
          : Date.now();
      const updated: TransferState = {
        ...state,
        progress: p,
        status: "transferring" as TransferStatus,
        aggregateBytesTransferred: aggregateBytes,
        aggregateTotalBytes: p.aggregate_total_bytes ?? p.total_bytes,
        currentFileIndex: p.file_index ?? 0,
        totalFiles: p.total_files ?? 1,
        transferStartTime: startTime,
        // 计算传输速度 (bytes/ms * 1000 = bytes/s)
        speed:
          (aggregateBytes / Math.max(1, Date.now() - startTime)) * 1000,
      };
      if (state.batchFiles[key]) {
        updated.batchFiles = {
          ...state.batchFiles,
          [key]: {
            ...state.batchFiles[key],
            bytesTransferred: p.bytes_transferred,
            totalBytes: p.total_bytes,
            status: "transferring" as const,
          },
        };
      } else {
        // 接收端事先不知道文件名，需按需创建条目
        updated.batchFiles = {
          ...state.batchFiles,
          [key]: {
            fileName: key,
            status: "transferring" as const,
            bytesTransferred: p.bytes_transferred,
            totalBytes: p.total_bytes,
          },
        };
      }
      return updated;
    }
    case "ADD_HISTORY":
      return {
        ...state,
        history: [action.item, ...state.history].slice(0, 100),
      };
    case "CLEAR_HISTORY":
      return { ...state, history: [] };
    case "SET_ERROR":
      return { ...state, error: action.error };
    case "SET_ACTIVE_PROTOCOL":
      return { ...state, activeProtocol: action.protocol };
    case "INIT_BATCH": {
      const batchFiles: Record<string, BatchFileEntry> = {};
      for (const name of action.fileNames) {
        batchFiles[name] = {
          fileName: name,
          status: "pending",
          bytesTransferred: 0,
          totalBytes: 0,
        };
      }
      return {
        ...state,
        batchFiles,
        totalFiles: action.fileNames.length,
        currentFileIndex: 0,
        aggregateBytesTransferred: 0,
        aggregateTotalBytes: 0,
        speed: 0,
        transferStartTime: Date.now(),
      };
    }
    case "FILE_START": {
      const key = action.event.file_name;
      const updatedBatch = { ...state.batchFiles };
      if (updatedBatch[key]) {
        updatedBatch[key] = {
          ...updatedBatch[key],
          status: "transferring",
          totalBytes: action.event.file_size,
        };
      } else {
        // 接收端按需创建条目
        updatedBatch[key] = {
          fileName: key,
          status: "transferring",
          bytesTransferred: 0,
          totalBytes: action.event.file_size,
        };
      }
      return {
        ...state,
        batchFiles: updatedBatch,
        currentFileIndex: action.event.file_index,
        totalFiles: action.event.total_files || state.totalFiles,
      };
    }
    case "FILE_COMPLETE": {
      const key = action.event.file_name;
      const updatedBatch = { ...state.batchFiles };
      if (updatedBatch[key]) {
        updatedBatch[key] = {
          ...updatedBatch[key],
          status: action.event.success ? "completed" : "failed",
          bytesTransferred: action.event.bytes_transferred,
          error: action.event.error ?? undefined,
        };
      } else {
        // 接收端按需创建条目
        updatedBatch[key] = {
          fileName: key,
          status: action.event.success ? "completed" : "failed",
          bytesTransferred: action.event.bytes_transferred,
          totalBytes: action.event.bytes_transferred,
          error: action.event.error ?? undefined,
        };
      }
      return {
        ...state,
        batchFiles: updatedBatch,
      };
    }
    case "SYNC_BATCH_RESULTS": {
      const synced = { ...state.batchFiles };
      for (const r of action.results) {
        if (synced[r.file_name]) {
          synced[r.file_name] = {
            ...synced[r.file_name],
            status: r.status as FileTransferState,
            bytesTransferred: r.size,
            totalBytes: r.size,
            error: r.error ?? undefined,
          };
        } else {
          // 接收端按需创建条目
          synced[r.file_name] = {
            fileName: r.file_name,
            status: r.status as FileTransferState,
            bytesTransferred: r.size,
            totalBytes: r.size,
            error: r.error ?? undefined,
          };
        }
      }
      for (const key of Object.keys(synced)) {
        if (synced[key].status === "pending") {
          synced[key] = {
            ...synced[key],
            status: "skipped",
            error: "Batch ended early, file not transferred",
          };
        }
      }
      return { ...state, batchFiles: synced };
    }
    case "RESET_BATCH":
      return {
        ...state,
        batchFiles: {},
        aggregateBytesTransferred: 0,
        aggregateTotalBytes: 0,
        currentFileIndex: 0,
        totalFiles: 0,
        speed: 0,
        transferStartTime: 0,
      };
    default:
      return state;
  }
}

// ── Context ───────────────────────────────────────────────

interface TransferContextValue {
  state: TransferState;
  /** 协议无关的统一传输入口 */
  startTransfer: (
    config: TransferConfig,
    sessionId: string,
    direction: TransferDirection,
    filePaths?: string[],
    downloadDir?: string,
  ) => Promise<void>;
  /** 便捷包装：YMODEM 发送 */
  sendFiles: (sessionId: string, filePaths: string[]) => Promise<void>;
  /** 便捷包装：YMODEM 接收 */
  receiveFiles: (sessionId: string, downloadDir: string) => Promise<void>;
  cancelTransfer: (sessionId: string) => Promise<void>;
  clearError: () => void;
  clearHistory: () => void;
}

const TransferContext = createContext<TransferContextValue | null>(null);

export function TransferProvider({ children }: { children: ReactNode }) {
  const [state, dispatch] = useReducer(transferReducer, initialState);
  const idCounter = useRef(0);
  // 使用 ref 追踪 activeProtocol，避免事件监听器闭包过期
  const activeProtocolRef = useRef(state.activeProtocol);
  activeProtocolRef.current = state.activeProtocol;

  const addHistory = useCallback(
    (item: Omit<TransferHistoryItem, "id">) => {
      dispatch({
        type: "ADD_HISTORY",
        item: { ...item, id: String(++idCounter.current) },
      });
    },
    [],
  );

  const extractFileNames = useCallback((filePaths: string[]): string[] => {
    return filePaths.map((p) => {
      const parts = p.replace(/\\/g, "/").split("/");
      return parts[parts.length - 1] || p;
    });
  }, []);

  // ── Protocol-agnostic startTransfer ─────────────────────

  const startTransfer = useCallback(
    async (
      config: TransferConfig,
      sessionId: string,
      direction: TransferDirection,
      filePaths?: string[],
      downloadDir?: string,
    ) => {
      const protocol = config.protocol;
      const commandName = COMMAND_MAP[protocol]?.[direction];
      if (!commandName) {
        dispatch({
          type: "SET_ERROR",
          error: "Unsupported transfer protocol or operation",
        });
        return;
      }

      dispatch({ type: "SET_ACTIVE_PROTOCOL", protocol });
      dispatch({ type: "SET_ERROR", error: null });

      if (direction === "send" && filePaths) {
        const fileNames = extractFileNames(filePaths);
        dispatch({ type: "INIT_BATCH", fileNames });
      } else if (direction === "receive") {
        dispatch({ type: "RESET_BATCH" });
      }

      dispatch({ type: "SET_STATUS", status: "transferring" });

      try {
        const args: Record<string, unknown> = {
          sessionId,
          protocol: config.protocol,
        };
        if (direction === "send" && filePaths) {
          args.filePaths = filePaths;
        }
        if (direction === "receive" && downloadDir) {
          args.downloadDir = downloadDir;
        }
        // 传递 YMODEM 专属配置到 Rust 端
        if (config.protocol === "ymodem" && "blockSize" in config) {
          args.blockSize = config.blockSize;
          args.checksumMode = config.checksumMode;
          args.streaming = (config as YmodemTransferConfig).streaming ?? false;
        }
        await invoke(commandName, args);
      } catch (e) {
        dispatch({ type: "SET_STATUS", status: "failed" });
        dispatch({
          type: "SET_ERROR",
          error: `Transfer failed: ${e}`,
        });
        addHistory({
          file_name:
            direction === "send"
              ? (filePaths && extractFileNames(filePaths).join(", ")) ||
                "unknown"
              : "batch-receive",
          direction,
          size: 0,
          status: "failed",
          timestamp: Date.now(),
          error: String(e),
          protocol,
        });
      }
    },
    [addHistory, extractFileNames],
  );

  // ── Convenience wrappers (backward-compatible YMODEM) ───

  const sendFiles = useCallback(
    async (sessionId: string, filePaths: string[]) => {
      return startTransfer(
        PROTOCOL_REGISTRY.ymodem.defaultConfig,
        sessionId,
        "send",
        filePaths,
      );
    },
    [startTransfer],
  );

  const receiveFiles = useCallback(
    async (sessionId: string, downloadDir: string) => {
      return startTransfer(
        PROTOCOL_REGISTRY.ymodem.defaultConfig,
        sessionId,
        "receive",
        undefined,
        downloadDir,
      );
    },
    [startTransfer],
  );

  const cancelTransfer = useCallback(async (sessionId: string) => {
    try {
      await invoke("cancel_transfer", { sessionId });
      dispatch({ type: "SET_STATUS", status: "cancelled" });
    } catch (e) {
      dispatch({ type: "SET_ERROR", error: `Cancel failed: ${e}` });
    }
  }, []);

  const clearError = useCallback(
    () => dispatch({ type: "SET_ERROR", error: null }),
    [],
  );
  const clearHistory = useCallback(
    () => dispatch({ type: "CLEAR_HISTORY" }),
    [],
  );
  // ── Event listeners ─────────────────────────────────────

  useEffect(() => {
    let cancelled = false;
    const unlisteners: UnlistenFn[] = [];

    (async () => {
      const u1 = await listen<TransferProgress>(
        "transfer-progress",
        (event) => {
          dispatch({ type: "SET_PROGRESS", progress: event.payload });
        },
      );
      if (cancelled) {
        u1();
        return;
      }
      unlisteners.push(u1);

      const u2 = await listen<TransferCompleteEvent>(
        "transfer-complete",
        (event) => {
          const payload = event.payload;
          if (payload.results && payload.results.length > 0) {
            dispatch({
              type: "SYNC_BATCH_RESULTS",
              results: payload.results,
            });
          }
          const hasFailures = (payload.files_failed ?? 0) > 0;
          const hasSkippedOnly =
            (payload.files_skipped ?? 0) > 0 && !hasFailures;
          if (payload.success || hasSkippedOnly) {
            dispatch({ type: "SET_STATUS", status: "completed" });
          } else {
            dispatch({ type: "SET_STATUS", status: "failed" });
          }
          // Per-file history with protocol info — 从 ref 读取避免闭包过期
          if (payload.results && payload.results.length > 0) {
            // use ref value; if already cleared, use state for correctness
            const activeProtocol = activeProtocolRef.current ?? state.activeProtocol;
            for (const r of payload.results) {
              const direction: TransferDirection =
                payload.direction ?? "send";
              addHistory({
                file_name: r.file_name,
                direction,
                size: r.size,
                status:
                  r.status === "completed" ? "completed" : "failed",
                timestamp: Date.now(),
                error: r.error ?? undefined,
                protocol: activeProtocol ?? state.activeProtocol ?? "unknown",
              });
            }
          }
        },
      );
      if (cancelled) {
        u2();
        return;
      }
      unlisteners.push(u2);

      const u3 = await listen<FileStartEvent>(
        "transfer-file-start",
        (event) => {
          dispatch({ type: "FILE_START", event: event.payload });
        },
      );
      if (cancelled) {
        u3();
        return;
      }
      unlisteners.push(u3);

      const u4 = await listen<FileCompleteEvent>(
        "transfer-file-complete",
        (event) => {
          dispatch({ type: "FILE_COMPLETE", event: event.payload });
        },
      );
      if (cancelled) {
        u4();
        return;
      }
      unlisteners.push(u4);

      // 监听会话断开，自动重置传输状态（避免残留进度和文件列表）
      const u5 = await listen<{ session_id: string }>(
        "session-disconnected",
        (_event) => {
          dispatch({ type: "RESET_BATCH" });
        },
      );
      if (cancelled) {
        u5();
        return;
      }
      unlisteners.push(u5);
    })().catch((e) => {
      console.error("TransferContext: Failed to register event listeners:", e);
    });

    return () => {
      cancelled = true;
      unlisteners.forEach((u) => u());
    };
  }, [addHistory]);

  return (
    <TransferContext.Provider
      value={{
        state,
        startTransfer,
        sendFiles,
        receiveFiles,
        cancelTransfer,
        clearError,
        clearHistory,
      }}
    >
      {children}
    </TransferContext.Provider>
  );
}

export function useTransfer() {
  const ctx = useContext(TransferContext);
  if (!ctx)
    throw new Error("useTransfer must be used within TransferProvider");
  return ctx;
}
