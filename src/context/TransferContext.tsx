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
  TransferStartAck,
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

export type ManagedTransferPhase =
  | "preparing"
  | "transferring"
  | "finalizing"
  | "cancelling"
  | "completed"
  | "failed"
  | "cancelled";

export interface ManagedTransferTask {
  transferId: string;
  sessionId: string;
  protocol: string;
  direction: "send" | "receive";
  phase: ManagedTransferPhase;
  fileName: string;
  bytesDone: number;
  bytesTotal: number;
  percent: number;
  speed: number | null;
  error: string | null;
  fileIndex: number;
  totalFiles: number;
  aggregateBytes: number;
  aggregateTotal: number;
}

interface TransferStartedEvent {
  session_id: string;
  transfer_id: string;
  protocol: string;
  direction: "send" | "receive";
}

interface UnifiedProgressEvent {
  session_id: string;
  transfer_id: string;
  protocol: string;
  file_name: string;
  bytes_done: number;
  bytes_total: number;
  bytes_per_second: number | null;
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

interface TransferFinishedEvent {
  session_id: string;
  transfer_id?: string;
  protocol?: string;
  success: boolean;
  cancelled?: boolean;
  error?: string | null;
}

export interface TransferState {
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
  /** 所有 Session 的最新传输任务快照。后端事件只在本 Context 中监听一次。 */
  tasksBySession: Record<string, ManagedTransferTask>;
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
  | { type: "SET_ACTIVE_SESSION_ID"; sessionId: string | null }
  | { type: "TASK_STARTED"; payload: TransferStartedEvent }
  | { type: "TASK_PROGRESS"; payload: UnifiedProgressEvent }
  | { type: "TASK_FINISHED"; payload: TransferFinishedEvent }
  | { type: "TASK_CANCELLING"; sessionId: string; transferId: string }
  | {
      type: "TASK_CANCEL_REJECTED";
      sessionId: string;
      transferId: string;
      previousPhase: ManagedTransferPhase;
      error: string;
    }
  | { type: "TASK_DISCARD"; sessionId: string; transferId: string }
  | { type: "TASK_DISCARD_SESSION"; sessionId: string };

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
  tasksBySession: {},
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
        // SFTP 等协议可直接提供真实 I/O 层测速；其它协议保留聚合平均速率回退。
        speed:
          p.bytes_per_second && p.bytes_per_second > 0
            ? p.bytes_per_second
            : (aggregateBytes / Math.max(1, Date.now() - startTime)) * 1000,
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
    case "TASK_STARTED": {
      const payload = action.payload;
      return {
        ...state,
        tasksBySession: {
          ...state.tasksBySession,
          [payload.session_id]: {
            transferId: payload.transfer_id,
            sessionId: payload.session_id,
            protocol: payload.protocol,
            direction: payload.direction,
            phase: "preparing",
            fileName: "",
            bytesDone: 0,
            bytesTotal: 0,
            percent: 0,
            speed: null,
            error: null,
            fileIndex: 0,
            totalFiles: 1,
            aggregateBytes: 0,
            aggregateTotal: 0,
          },
        },
      };
    }
    case "TASK_PROGRESS": {
      const payload = action.payload;
      const current = state.tasksBySession[payload.session_id];
      if (!current || current.transferId !== payload.transfer_id) return state;

      const knownTotal = payload.bytes_total > 0;
      const isLastFile =
        payload.total_files <= 1 || payload.file_index + 1 >= payload.total_files;
      const percent = knownTotal
        ? payload.bytes_done >= payload.bytes_total
          ? 100
          : Math.min(
              99,
              Math.max(0, Math.floor((payload.bytes_done / payload.bytes_total) * 100)),
            )
        : current.percent;
      let phase: ManagedTransferPhase = current.phase;
      // 取消请求一旦被后端接受，就保持 cancelling，直到精确 transfer_id 的 finished。
      // 迟到 progress 不能把 UI 又退回 transferring/finalizing。
      if (current.phase === "cancelling") {
        phase = "cancelling";
      } else if (payload.is_batch_complete) {
        phase = "finalizing";
      } else if (payload.is_file_start) {
        phase = "transferring";
      } else if (payload.is_file_complete) {
        phase = isLastFile ? "finalizing" : "transferring";
      } else {
        phase = knownTotal && payload.bytes_done >= payload.bytes_total && isLastFile
          ? "finalizing"
          : "transferring";
      }

      const preserveFailedProgress =
        payload.is_file_complete && payload.file_success === false && !knownTotal;
      const measuredSpeed =
        typeof payload.bytes_per_second === "number"
        && Number.isFinite(payload.bytes_per_second)
        && payload.bytes_per_second > 0
          ? payload.bytes_per_second
          : null;
      const nextSpeed = measuredSpeed
        ?? (payload.is_file_start ? null : current.speed);
      const nextTask: ManagedTransferTask = {
        ...current,
        direction: payload.direction,
        phase,
        fileName: payload.is_batch_complete
          ? current.fileName
          : (payload.file_name === "__batch_complete__" ? current.fileName : payload.file_name),
        bytesDone:
          payload.is_batch_complete || preserveFailedProgress
            ? current.bytesDone
            : payload.bytes_done,
        bytesTotal:
          payload.is_batch_complete || preserveFailedProgress
            ? current.bytesTotal
            : payload.bytes_total,
        percent:
          payload.is_batch_complete || preserveFailedProgress
            ? current.percent
            : percent,
        // Keep the last reliable I/O sample through finalizing so the short-lived
        // completed card can show a useful throughput summary instead of dropping
        // directly from live speed to no speed at 100%.
        speed: nextSpeed,
        error: payload.file_success === false
          ? (payload.file_error || current.error)
          : current.error,
        fileIndex: payload.is_batch_complete ? current.fileIndex : payload.file_index,
        totalFiles: payload.is_batch_complete
          ? current.totalFiles
          : (payload.total_files || current.totalFiles),
        aggregateBytes: payload.is_batch_complete
          ? current.aggregateBytes
          : payload.aggregate_bytes,
        aggregateTotal: payload.is_batch_complete
          ? current.aggregateTotal
          : (payload.aggregate_total || current.aggregateTotal),
      };
      return {
        ...state,
        tasksBySession: {
          ...state.tasksBySession,
          [payload.session_id]: nextTask,
        },
      };
    }
    case "TASK_FINISHED": {
      const payload = action.payload;
      if (!payload.transfer_id) return state;
      const current = state.tasksBySession[payload.session_id];
      if (!current || current.transferId !== payload.transfer_id) return state;
      if (payload.protocol && current.protocol !== payload.protocol) return state;
      const phase: ManagedTransferPhase = payload.success
        ? "completed"
        : payload.cancelled
          ? "cancelled"
          : "failed";
      return {
        ...state,
        tasksBySession: {
          ...state.tasksBySession,
          [payload.session_id]: {
            ...current,
            phase,
            percent: payload.success ? 100 : current.percent,
            speed: payload.success ? current.speed : null,
            error: payload.success ? null : (payload.error || current.error),
          },
        },
      };
    }
    case "TASK_CANCELLING": {
      const current = state.tasksBySession[action.sessionId];
      if (!current || current.transferId !== action.transferId) return state;
      return {
        ...state,
        tasksBySession: {
          ...state.tasksBySession,
          [action.sessionId]: {
            ...current,
            phase: "cancelling",
            speed: null,
          },
        },
      };
    }
    case "TASK_CANCEL_REJECTED": {
      const current = state.tasksBySession[action.sessionId];
      if (
        !current
        || current.transferId !== action.transferId
        || current.phase !== "cancelling"
      ) {
        return state;
      }
      return {
        ...state,
        tasksBySession: {
          ...state.tasksBySession,
          [action.sessionId]: {
            ...current,
            phase: action.previousPhase,
            error: action.error,
          },
        },
      };
    }
    case "TASK_DISCARD": {
      const current = state.tasksBySession[action.sessionId];
      if (!current || current.transferId !== action.transferId) return state;
      const tasksBySession = { ...state.tasksBySession };
      delete tasksBySession[action.sessionId];
      return { ...state, tasksBySession };
    }
    case "TASK_DISCARD_SESSION": {
      if (!state.tasksBySession[action.sessionId]) return state;
      const tasksBySession = { ...state.tasksBySession };
      delete tasksBySession[action.sessionId];
      return { ...state, tasksBySession };
    }
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
  cancelTask: (sessionId: string, transferId: string) => Promise<void>;
  dismissTask: (sessionId: string, transferId: string) => void;
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
  // 每个已启动传输都有独立 transfer_id；用于拒绝同 Session 的迟到事件。
  const activeTransferIdRef = useRef<string | null>(null);
  const activeDirectionRef = useRef<TransferDirection | null>(null);
  const lastAggregateBytesRef = useRef(0);
  // Inline 传输的 invoke 会在真实传输结束后才返回；用于避免 finished + invoke catch 双重终态。
  const backendStartedRef = useRef(false);

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

      // started 事件可能早于下一次 React render；先同步 refs，再 dispatch UI state。
      activeProtocolRef.current = protocol;
      activeSessionIdRef.current = sessionId;
      activeTransferIdRef.current = null;
      activeDirectionRef.current = direction;
      lastAggregateBytesRef.current = 0;
      backendStartedRef.current = false;
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
        const ack = await invoke<TransferStartAck>(commandName, { request: args });
        if (
          activeSessionIdRef.current === sessionId
          && activeProtocolRef.current === protocol
          && ack?.transfer_id
        ) {
          activeTransferIdRef.current = ack.transfer_id;
        }
      } catch (e) {
        console.error(`[TransferContext] ${commandName} failed:`, e);
        // 已收到 started 的传输，其终态由精确 transfer_id 的 finished 唯一负责。
        // Inline 路径会在 emit finished 后让 invoke reject，不能在这里重复记失败。
        if (backendStartedRef.current) {
          return;
        }
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
        activeProtocolRef.current = null;
        activeSessionIdRef.current = null;
        activeDirectionRef.current = null;
        lastAggregateBytesRef.current = 0;
        dispatch({ type: "SET_ACTIVE_PROTOCOL", protocol: null });
        dispatch({ type: "SET_ACTIVE_SESSION_ID", sessionId: null });
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

  const cancelTask = useCallback(async (sessionId: string, transferId: string) => {
    const current = state.tasksBySession[sessionId];
    if (!current || current.transferId !== transferId) {
      throw new Error("Transfer task is no longer active");
    }
    if (
      current.phase === "completed"
      || current.phase === "failed"
      || current.phase === "cancelled"
    ) {
      return;
    }

    const previousPhase = current.phase;
    // 先进入 cancelling，再发送命令。这样即使 finished 比 invoke resolve 更早到达，
    // 后续也不会再有“成功返回后把终态倒退为 cancelling”的窗口。
    dispatch({ type: "TASK_CANCELLING", sessionId, transferId });
    try {
      await invoke("file_transfer_cancel", { sessionId, transferId });
    } catch (error) {
      dispatch({
        type: "TASK_CANCEL_REJECTED",
        sessionId,
        transferId,
        previousPhase,
        error: String(error),
      });
      throw error;
    }
  }, [state.tasksBySession]);

  const dismissTask = useCallback((sessionId: string, transferId: string) => {
    dispatch({ type: "TASK_DISCARD", sessionId, transferId });
  }, []);

  const cancelTransfer = useCallback(async (sessionId: string) => {
    const transferId = activeTransferIdRef.current;
    if (!transferId) return;
    try {
      await cancelTask(sessionId, transferId);
      // 取消命令仅代表请求被接受；真正 cancelled 终态仍只由精确 transfer_id 的
      // file-transfer:finished 事件决定。
    } catch (e) {
      dispatch({ type: "SET_ERROR", error: `Cancel failed: ${e}` });
    }
  }, [cancelTask]);

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
          // 统一任务存储按 Session 管理，任何断开都必须丢弃该 Session 的任务快照；
          // legacy Transmission 面板的 owner 状态则只在命中 activeSession 时重置。
          dispatch({ type: "TASK_DISCARD_SESSION", sessionId: event.payload.session_id });
          if (event.payload.session_id !== activeSessionIdRef.current) return;
          activeTransferIdRef.current = null;
          activeDirectionRef.current = null;
          lastAggregateBytesRef.current = 0;
          backendStartedRef.current = false;
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

      // started 是统一任务存储的身份源；legacy 传输面板随后按当前 owner 过滤。
      const uStarted = await listen<TransferStartedEvent>(
        "file-transfer:started",
        (event) => {
          const payload = event.payload;
          dispatch({ type: "TASK_STARTED", payload });
          if (payload.session_id !== activeSessionIdRef.current) return;
          if (payload.protocol !== activeProtocolRef.current) return;
          backendStartedRef.current = true;
          activeTransferIdRef.current = payload.transfer_id;
        },
      );
      if (cancelled) {
        uStarted();
        return;
      }
      unlisteners.push(uStarted);

      // ── 统一进度事件 ──
      const u2 = await listen<UnifiedProgressEvent>(
        "file-transfer:progress",
        (event) => {
          const p = event.payload;
          dispatch({ type: "TASK_PROGRESS", payload: p });
          // legacy 传输面板同时按 Session / protocol / transfer_id 过滤。
          if (p.session_id !== activeSessionIdRef.current) return;
          if (p.protocol !== activeProtocolRef.current) return;
          if (!activeTransferIdRef.current || p.transfer_id !== activeTransferIdRef.current) return;
          // batch_complete 只是协议层批次收尾；真正终态由 finished 统一决定。
          // 不在这里提前 completed/failed，避免跳过资源释放阶段，也避免把取消误判为失败。
          if (p.is_batch_complete) {
            return;
          }

          lastAggregateBytesRef.current = p.aggregate_bytes;
          const progress: TransferProgress = {
            file_name: p.file_name,
            bytes_transferred: p.bytes_done,
            total_bytes: p.bytes_total,
            file_index: p.file_index,
            total_files: p.total_files,
            aggregate_bytes_transferred: p.aggregate_bytes,
            aggregate_total_bytes: p.aggregate_total,
            bytes_per_second:
              typeof p.bytes_per_second === "number" && p.bytes_per_second > 0
                ? p.bytes_per_second
                : undefined,
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

      const uFinished = await listen<TransferFinishedEvent>(
        "file-transfer:finished",
        (event) => {
          const payload = event.payload;
          dispatch({ type: "TASK_FINISHED", payload });
          if (payload.session_id !== activeSessionIdRef.current) return;
          if (payload.protocol && payload.protocol !== activeProtocolRef.current) return;
          if (
            !payload.transfer_id
            || !activeTransferIdRef.current
            || payload.transfer_id !== activeTransferIdRef.current
          ) {
            return;
          }

          const status: TransferStatus = payload.success
            ? "completed"
            : payload.cancelled
              ? "cancelled"
              : "failed";
          dispatch({ type: "SET_STATUS", status });
          dispatch({
            type: "SET_ERROR",
            error: payload.success ? null : (payload.error ?? "Transfer failed"),
          });
          addHistory({
            file_name: "batch",
            direction: activeDirectionRef.current ?? "send",
            size: lastAggregateBytesRef.current,
            status,
            timestamp: Date.now(),
            error: payload.success ? undefined : (payload.error ?? undefined),
            protocol: activeProtocolRef.current ?? "unknown",
          });

          activeTransferIdRef.current = null;
          activeDirectionRef.current = null;
          lastAggregateBytesRef.current = 0;
          activeProtocolRef.current = null;
          activeSessionIdRef.current = null;
          dispatch({ type: "SET_ACTIVE_PROTOCOL", protocol: null });
          dispatch({ type: "SET_ACTIVE_SESSION_ID", sessionId: null });
        },
      );
      if (cancelled) {
        uFinished();
        return;
      }
      unlisteners.push(uFinished);
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
        cancelTask,
        dismissTask,
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
