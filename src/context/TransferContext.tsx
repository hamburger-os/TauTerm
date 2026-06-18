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

/** 协议 → 方向 → Tauri 命令名 */
const COMMAND_MAP: Record<
  ProtocolType,
  Record<TransferDirection, string>
> = {
  ymodem: {
    send: "send_files_ymodem",
    receive: "receive_files_ymodem",
  },
  xmodem: {
    send: "send_files_xmodem",
    receive: "receive_files_xmodem",
  },
  zmodem: {
    send: "send_files_zmodem",
    receive: "receive_files_zmodem",
  },
};

// ── State ─────────────────────────────────────────────────

interface TransferState {
  status: TransferStatus;
  progress: TransferProgress | null;
  history: TransferHistoryItem[];
  error: string | null;
  isDragging: boolean;
  /** 批次文件追踪 Map<fileName, BatchFileEntry> */
  batchFiles: Record<string, BatchFileEntry>;
  /** 批次聚合进度 */
  aggregateBytesTransferred: number;
  aggregateTotalBytes: number;
  currentFileIndex: number;
  totalFiles: number;
  /** 当前活跃传输使用的协议 */
  activeProtocol: ProtocolType | null;
}

type TransferAction =
  | { type: "SET_STATUS"; status: TransferStatus }
  | { type: "SET_PROGRESS"; progress: TransferProgress }
  | { type: "ADD_HISTORY"; item: TransferHistoryItem }
  | { type: "CLEAR_HISTORY" }
  | { type: "SET_ERROR"; error: string | null }
  | { type: "SET_DRAGGING"; dragging: boolean }
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
  isDragging: false,
  batchFiles: {},
  aggregateBytesTransferred: 0,
  aggregateTotalBytes: 0,
  currentFileIndex: 0,
  totalFiles: 0,
  activeProtocol: null,
};

function transferReducer(
  state: TransferState,
  action: TransferAction,
): TransferState {
  switch (action.type) {
    case "SET_STATUS":
      return { ...state, status: action.status };
    case "SET_PROGRESS": {
      const p = action.progress;
      const key = p.file_name;
      const updated: TransferState = {
        ...state,
        progress: p,
        status: "transferring" as TransferStatus,
        aggregateBytesTransferred:
          p.aggregate_bytes_transferred ?? p.bytes_transferred,
        aggregateTotalBytes: p.aggregate_total_bytes ?? p.total_bytes,
        currentFileIndex: p.file_index ?? 0,
        totalFiles: p.total_files ?? 1,
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
    case "SET_DRAGGING":
      return { ...state, isDragging: action.dragging };
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
  setDragging: (dragging: boolean) => void;
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
          error: "transfer.error.unsupported",
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
        const args: Record<string, unknown> = { sessionId };
        if (direction === "send" && filePaths) {
          args.filePaths = filePaths;
        }
        if (direction === "receive" && downloadDir) {
          args.downloadDir = downloadDir;
        }
        // Pass protocol config params to backend
        switch (config.protocol) {
          case "ymodem":
            args.blockSize = config.blockSize;
            args.checksumMode = config.checksumMode;
            break;
          case "xmodem":
            args.blockSize = config.blockSize;
            args.checksumMode = config.checksumMode;
            args.initChar = config.initChar;
            break;
          case "zmodem":
            args.windowSize = config.windowSize;
            args.resume = config.resumeEnabled;
            args.compression = config.compressionEnabled;
            break;
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
  const setDragging = useCallback(
    (dragging: boolean) => dispatch({ type: "SET_DRAGGING", dragging }),
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
            const activeProtocol = activeProtocolRef.current ?? "ymodem";
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
                protocol: activeProtocol ?? "ymodem",
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
        setDragging,
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
