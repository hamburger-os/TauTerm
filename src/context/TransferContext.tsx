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

/** 协议 → 方向 → Tauri 命令名（统一使用 file_transfer_send / file_transfer_receive） */
const COMMAND_MAP: Record<
  ProtocolType,
  Record<TransferDirection, string>
> = {
  ymodem: {
    send: "file_transfer_send",
    receive: "file_transfer_receive",
  },
  xmodem: {
    send: "file_transfer_send",
    receive: "file_transfer_receive",
  },
  zmodem: {
    send: "file_transfer_send",
    receive: "file_transfer_receive",
  },
  sftp: {
    send: "file_transfer_send",
    receive: "file_transfer_receive",
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
  /** 当前活跃传输所属会话 ID（用于过滤跨会话进度事件） */
  activeSessionId: string | null;
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
  | { type: "SYNC_BATCH_RESULTS"; results: BatchFileResult[] }
  | { type: "RESET_BATCH" }
  | { type: "SET_ACTIVE_PROTOCOL"; protocol: ProtocolType | null }
  | { type: "SET_ACTIVE_SESSION_ID"; sessionId: string | null };

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
  activeSessionId: null,
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
      // Bug #1 双重防御：跳过空文件名事件，防止创建幽灵条目
      const key = p.file_name;
      if (!key || key.trim() === "") {
        return {
          ...state,
          progress: p,
          aggregateBytesTransferred:
            p.aggregate_bytes_transferred ?? p.bytes_transferred,
          aggregateTotalBytes: p.aggregate_total_bytes ?? p.total_bytes,
          currentFileIndex: p.file_index ?? 0,
          totalFiles: p.total_files ?? 1,
        };
      }
      const aggregateBytes =
        p.aggregate_bytes_transferred ?? p.bytes_transferred;
      // 接收端无 INIT_BATCH，首次 progress 事件时初始化计时起点
      const startTime =
        state.transferStartTime > 0
          ? state.transferStartTime
          : Date.now();
      const isTerminal = state.status === "completed"
        || state.status === "failed"
        || state.status === "cancelled";
      const updated: TransferState = {
        ...state,
        progress: p,
        status: isTerminal ? state.status : ("transferring" as TransferStatus),
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
    case "SET_ACTIVE_SESSION_ID":
      return { ...state, activeSessionId: action.sessionId };
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
  // 使用 ref 追踪 activeSessionId，避免事件监听器闭包过期（同 activeProtocolRef 模式）
  const activeSessionIdRef = useRef(state.activeSessionId);
  activeSessionIdRef.current = state.activeSessionId;

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
      dispatch({ type: "SET_ACTIVE_SESSION_ID", sessionId });
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
        if (direction === "receive") {
          args.remotePaths = []; // 串口协议由发送端决定文件列表；SFTP 路径由 FileManager 直接传参
          if (downloadDir) {
            args.downloadDir = downloadDir;
          }
        }
        // 传递 YMODEM 专属配置
        if (config.protocol === "ymodem" && "blockSize" in config) {
          args.blockSize = config.blockSize;
          args.checksumMode = config.checksumMode;
          args.streaming = (config as YmodemTransferConfig).streaming ?? false;
        }
        if (import.meta.env.DEV) {
          console.log(
            `[TransferContext] invoking ${commandName} direction=${direction} protocol=${config.protocol}`,
            JSON.stringify(args, null, 2),
          );
        }
        await invoke(commandName, args);
      } catch (e) {
        console.error(`[TransferContext] ${commandName} failed:`, e);
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
      await invoke("file_transfer_cancel", { sessionId });
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
      // 监听会话断开，自动重置传输状态（避免残留进度和文件列表）
      const u1 = await listen<{ session_id: string }>(
        "session-disconnected",
        (event) => {
          if (event.payload.session_id !== activeSessionIdRef.current) return;
          dispatch({ type: "RESET_BATCH" });
          dispatch({ type: "SET_ACTIVE_SESSION_ID", sessionId: null });
          dispatch({ type: "SET_STATUS", status: "idle" });
        },
      );
      if (cancelled) {
        u1();
        return;
      }
      unlisteners.push(u1);

      // ── 统一进度事件（替代 transfer-progress + sftp-progress 双轨制）──
      interface UnifiedProgressPayload {
        session_id: string;
        protocol: string;
        file_name: string;
        bytes_done: number;
        bytes_total: number;
        file_index: number;
        total_files: number;
        aggregate_bytes: number;
        aggregate_total: number;
        direction: "send" | "receive";
        is_file_start: boolean;
        is_file_complete: boolean;
        file_success: boolean | null;
        file_error: string | null;
        is_batch_complete: boolean;
      }
      const u2 = await listen<UnifiedProgressPayload>(
        "file-transfer:progress",
        (event) => {
          const p = event.payload;
          // 过滤跨会话进度事件：仅处理当前活跃会话的传输进度
          if (p.session_id !== activeSessionIdRef.current) return;
          // 映射到现有 TransferProgress 格式
          // batch_complete 事件只更新状态，不 dispatch SET_PROGRESS
          // 避免空 file_name 创建幽灵条目（Bug #1）
          if (p.is_batch_complete) {
            // file_success 在 batch_complete 中基于 files_failed/files_skipped 设置
            const ok = p.file_success !== false;
            dispatch({
              type: "SET_STATUS",
              status: ok ? "completed" : "failed",
            });
            addHistory({
              file_name: p.file_name !== "__batch_complete__" ? p.file_name : "batch",
              direction: p.direction,
              size: p.aggregate_bytes,
              status: ok ? "completed" : "failed",
              timestamp: Date.now(),
              error: p.file_error ?? undefined,
              protocol: activeProtocolRef.current ?? "unknown",
            });
            return;
          }

          const progress: TransferProgress = {
            file_name: p.file_name,
            bytes_transferred: p.bytes_done,
            total_bytes: p.bytes_total,
            file_index: p.file_index,
            total_files: p.total_files,
            aggregate_bytes_transferred: p.aggregate_bytes,
            aggregate_total_bytes: p.aggregate_total,
            direction: p.direction,
          };
          dispatch({ type: "SET_PROGRESS", progress });

          // 文件级别事件通过 SET_PROGRESS 自然处理
        },
      );
      if (cancelled) {
        u2();
        return;
      }
      unlisteners.push(u2);
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
