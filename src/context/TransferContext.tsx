import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useReducer,
  type ReactNode,
} from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  cancelFileTransfer,
  startFileTransfer,
} from "../services/transferService";
import type {
  BatchFileEntry,
  FileTransferState,
  ProtocolType,
  TransferConfig,
  TransferDirection,
  TransferFinishedPayload,
  TransferHistoryItem,
  TransferStartedPayload,
  TransferStartAck,
  TransferStatus,
  UnifiedTransferProgressPayload,
  YmodemTransferConfig,
} from "../types/transfer";
import { PROTOCOL_REGISTRY } from "../types/transfer";

export type {
  BatchFileEntry,
  TransferDirection,
  TransferHistoryItem,
  TransferStatus,
};

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
  direction: TransferDirection;
  phase: ManagedTransferPhase;
  fileName: string;
  bytesDone: number;
  bytesTotal: number;
  percent: number;
  speed: number | null;
  startedAt: number;
  /** Successful-task completion timestamp used only for UI retention/cleanup. */
  completedAt: number | null;
  error: string | null;
  fileIndex: number;
  totalFiles: number;
  aggregateBytes: number;
  aggregateTotal: number;
  files: BatchFileEntry[];
}

export interface TransferState {
  /** transfer_id 是前端任务存储的唯一主键。 */
  tasksById: Record<string, ManagedTransferTask>;
  /** Session 仅保存任务 ID 索引；当前后端默认 1 个活动任务，但模型允许未来扩展。 */
  taskIdsBySession: Record<string, string[]>;
  /** 启动阶段尚未产生 transfer_id 时的 Session 级错误。 */
  startErrorsBySession: Record<string, string>;
  /** 仅供应用级 Toast 投影最近一次错误；不参与任务所有权或生命周期判断。 */
  error: string | null;
  history: TransferHistoryItem[];
}

type TransferAction =
  | { type: "TASK_STARTED"; payload: TransferStartedPayload }
  | { type: "TASK_PROGRESS"; payload: UnifiedTransferProgressPayload }
  | { type: "TASK_FINISHED"; payload: TransferFinishedPayload }
  | { type: "TASK_CANCELLING"; sessionId: string; transferId: string }
  | {
      type: "TASK_CANCEL_REJECTED";
      sessionId: string;
      transferId: string;
      previousPhase: ManagedTransferPhase;
      error: string;
    }
  | { type: "TASK_START_FAILED"; sessionId: string; error: string }
  | { type: "TASK_CLEAR_ERROR"; sessionId?: string }
  | { type: "TASK_DISCARD"; sessionId: string; transferId: string }
  | { type: "TASK_DISCARD_SESSION"; sessionId: string }
  | { type: "CLEAR_HISTORY" };

const initialState: TransferState = {
  tasksById: {},
  taskIdsBySession: {},
  startErrorsBySession: {},
  error: null,
  history: [],
};

export function isManagedTransferTerminalPhase(phase: ManagedTransferPhase): boolean {
  return phase === "completed" || phase === "failed" || phase === "cancelled";
}

function knownHistoryProtocol(protocol: string): ProtocolType | "unknown" {
  return Object.prototype.hasOwnProperty.call(PROTOCOL_REGISTRY, protocol)
    ? (protocol as ProtocolType)
    : "unknown";
}

function createTask(
  sessionId: string,
  transferId: string,
  protocol: string,
  direction: TransferDirection,
): ManagedTransferTask {
  return {
    transferId,
    sessionId,
    protocol,
    direction,
    phase: "preparing",
    fileName: "",
    bytesDone: 0,
    bytesTotal: 0,
    percent: 0,
    speed: null,
    startedAt: Date.now(),
    completedAt: null,
    error: null,
    fileIndex: 0,
    totalFiles: 1,
    aggregateBytes: 0,
    aggregateTotal: 0,
    files: [],
  };
}

function addTaskToSessionIndex(
  taskIdsBySession: Record<string, string[]>,
  sessionId: string,
  transferId: string,
): Record<string, string[]> {
  const current = taskIdsBySession[sessionId] ?? [];
  if (current.includes(transferId)) return taskIdsBySession;
  return {
    ...taskIdsBySession,
    [sessionId]: [...current, transferId],
  };
}

function latestTaskForSession(
  state: TransferState,
  sessionId: string,
): ManagedTransferTask | undefined {
  const ids = state.taskIdsBySession[sessionId] ?? [];
  for (let i = ids.length - 1; i >= 0; i -= 1) {
    const task = state.tasksById[ids[i]];
    if (task) return task;
  }
  return undefined;
}

function latestActiveTaskForSession(
  state: TransferState,
  sessionId: string,
): ManagedTransferTask | undefined {
  const ids = state.taskIdsBySession[sessionId] ?? [];
  for (let i = ids.length - 1; i >= 0; i -= 1) {
    const task = state.tasksById[ids[i]];
    if (task && !isManagedTransferTerminalPhase(task.phase)) return task;
  }
  return undefined;
}

function updateFileProjection(
  current: ManagedTransferTask,
  payload: UnifiedTransferProgressPayload,
): BatchFileEntry[] {
  if (
    payload.is_batch_complete
    || !payload.file_name
    || payload.file_name === "__batch_complete__"
  ) {
    return current.files;
  }

  const index = Math.max(0, payload.file_index);
  const next = [...current.files];
  const existing = next[index];
  let status: FileTransferState = "transferring";
  if (payload.is_file_complete) {
    status = payload.file_success === false ? "failed" : "completed";
  }

  next[index] = {
    fileName: payload.file_name,
    status,
    bytesTransferred: payload.bytes_done,
    totalBytes: payload.bytes_total,
    error: payload.file_error ?? existing?.error,
  };
  return next;
}

function resultProjection(payload: TransferFinishedPayload): BatchFileEntry[] | null {
  if (!payload.results) return null;
  return payload.results.map((result) => ({
    fileName: result.file_name,
    status: result.status,
    bytesTransferred: result.size,
    totalBytes: result.size,
    error: result.error ?? undefined,
  }));
}

function transferReducer(state: TransferState, action: TransferAction): TransferState {
  switch (action.type) {
    case "TASK_STARTED": {
      const payload = action.payload;
      if (state.tasksById[payload.transfer_id]) {
        return state;
      }

      const oldIds = state.taskIdsBySession[payload.session_id] ?? [];
      const retainedIds = oldIds.filter((id) => {
        const task = state.tasksById[id];
        return task && !isManagedTransferTerminalPhase(task.phase);
      });
      const tasksById = { ...state.tasksById };
      for (const id of oldIds) {
        if (!retainedIds.includes(id)) delete tasksById[id];
      }
      tasksById[payload.transfer_id] = createTask(
        payload.session_id,
        payload.transfer_id,
        payload.protocol,
        payload.direction,
      );
      const startErrorsBySession = { ...state.startErrorsBySession };
      delete startErrorsBySession[payload.session_id];
      return {
        ...state,
        tasksById,
        taskIdsBySession: {
          ...state.taskIdsBySession,
          [payload.session_id]: [...retainedIds, payload.transfer_id],
        },
        startErrorsBySession,
        error: null,
      };
    }

    case "TASK_PROGRESS": {
      const payload = action.payload;
      let current = state.tasksById[payload.transfer_id];
      let tasksById = state.tasksById;
      let taskIdsBySession = state.taskIdsBySession;

      if (!current) {
        current = createTask(
          payload.session_id,
          payload.transfer_id,
          payload.protocol,
          payload.direction,
        );
        tasksById = {
          ...tasksById,
          [payload.transfer_id]: current,
        };
        taskIdsBySession = addTaskToSessionIndex(
          taskIdsBySession,
          payload.session_id,
          payload.transfer_id,
        );
      }
      if (
        current.sessionId !== payload.session_id
        || current.protocol !== payload.protocol
      ) {
        return state;
      }

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

      let phase: ManagedTransferPhase;
      if (current.phase === "cancelling") {
        phase = "cancelling";
      } else if (payload.is_batch_complete) {
        phase = "finalizing";
      } else if (payload.is_file_complete) {
        phase = isLastFile ? "finalizing" : "transferring";
      } else {
        phase = "transferring";
      }

      const measuredSpeed =
        typeof payload.bytes_per_second === "number"
        && Number.isFinite(payload.bytes_per_second)
        && payload.bytes_per_second > 0
          ? payload.bytes_per_second
          : null;
      const elapsedSeconds = Math.max(0.001, (Date.now() - current.startedAt) / 1000);
      const fallbackSpeed = payload.aggregate_bytes > 0
        ? payload.aggregate_bytes / elapsedSeconds
        : null;
      const nextSpeed = measuredSpeed
        ?? (payload.is_file_start ? null : current.speed ?? fallbackSpeed);
      const preserveFailedProgress =
        payload.is_file_complete && payload.file_success === false && !knownTotal;

      const nextTask: ManagedTransferTask = {
        ...current,
        direction: payload.direction,
        phase,
        fileName: payload.is_batch_complete
          ? current.fileName
          : payload.file_name === "__batch_complete__"
            ? current.fileName
            : payload.file_name,
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
        files: updateFileProjection(current, payload),
      };

      return {
        ...state,
        tasksById: {
          ...tasksById,
          [payload.transfer_id]: nextTask,
        },
        taskIdsBySession,
      };
    }

    case "TASK_FINISHED": {
      const payload = action.payload;
      if (!payload.transfer_id) return state;
      const current = state.tasksById[payload.transfer_id];
      if (!current || current.sessionId !== payload.session_id) return state;
      if (payload.protocol && current.protocol !== payload.protocol) return state;
      if (isManagedTransferTerminalPhase(current.phase)) return state;

      const phase: ManagedTransferPhase = payload.success
        ? "completed"
        : payload.cancelled
          ? "cancelled"
          : "failed";
      const exactResults = resultProjection(payload);
      const files = exactResults ?? current.files.map((entry) => {
        if (phase === "cancelled" && entry.status === "transferring") {
          return {
            ...entry,
            status: "skipped" as const,
            error: entry.error ?? "Transfer cancelled",
          };
        }
        return entry;
      });
      const error = payload.success ? null : (payload.error || current.error);
      const nextTask: ManagedTransferTask = {
        ...current,
        phase,
        percent: payload.success ? 100 : current.percent,
        speed: payload.success ? current.speed : null,
        completedAt: payload.success ? Date.now() : null,
        error,
        files,
      };
      const status: TransferStatus = payload.success
        ? "completed"
        : payload.cancelled
          ? "cancelled"
          : "failed";
      const historyItem: TransferHistoryItem = {
        id: current.transferId,
        file_name: files.length === 1 ? files[0].fileName : "batch",
        direction: current.direction,
        size: current.aggregateBytes,
        status,
        timestamp: Date.now(),
        error: error ?? undefined,
        protocol: knownHistoryProtocol(current.protocol),
      };

      return {
        ...state,
        tasksById: {
          ...state.tasksById,
          [payload.transfer_id]: nextTask,
        },
        error,
        history: [historyItem, ...state.history].slice(0, 100),
      };
    }

    case "TASK_CANCELLING": {
      const current = state.tasksById[action.transferId];
      if (!current || current.sessionId !== action.sessionId) return state;
      if (isManagedTransferTerminalPhase(current.phase)) return state;
      return {
        ...state,
        tasksById: {
          ...state.tasksById,
          [action.transferId]: {
            ...current,
            phase: "cancelling",
            speed: null,
            error: null,
          },
        },
      };
    }

    case "TASK_CANCEL_REJECTED": {
      const current = state.tasksById[action.transferId];
      if (
        !current
        || current.sessionId !== action.sessionId
        || current.phase !== "cancelling"
      ) {
        return state;
      }
      return {
        ...state,
        tasksById: {
          ...state.tasksById,
          [action.transferId]: {
            ...current,
            phase: action.previousPhase,
            error: action.error,
          },
        },
        error: action.error,
      };
    }

    case "TASK_START_FAILED":
      return {
        ...state,
        startErrorsBySession: {
          ...state.startErrorsBySession,
          [action.sessionId]: action.error,
        },
        error: action.error,
      };

    case "TASK_CLEAR_ERROR": {
      if (!action.sessionId) {
        const tasksById = Object.fromEntries(
          Object.entries(state.tasksById).map(([id, task]) => [id, { ...task, error: null }]),
        );
        return {
          ...state,
          tasksById,
          startErrorsBySession: {},
          error: null,
        };
      }
      const startErrorsBySession = { ...state.startErrorsBySession };
      delete startErrorsBySession[action.sessionId];
      const latest = latestTaskForSession(state, action.sessionId);
      if (!latest || !latest.error) {
        return { ...state, startErrorsBySession, error: null };
      }
      return {
        ...state,
        startErrorsBySession,
        tasksById: {
          ...state.tasksById,
          [latest.transferId]: { ...latest, error: null },
        },
        error: null,
      };
    }

    case "TASK_DISCARD": {
      const current = state.tasksById[action.transferId];
      if (!current || current.sessionId !== action.sessionId) return state;
      const tasksById = { ...state.tasksById };
      delete tasksById[action.transferId];
      const remaining = (state.taskIdsBySession[action.sessionId] ?? [])
        .filter((id) => id !== action.transferId);
      const taskIdsBySession = { ...state.taskIdsBySession };
      if (remaining.length > 0) taskIdsBySession[action.sessionId] = remaining;
      else delete taskIdsBySession[action.sessionId];
      return { ...state, tasksById, taskIdsBySession };
    }

    case "TASK_DISCARD_SESSION": {
      const ids = state.taskIdsBySession[action.sessionId] ?? [];
      if (ids.length === 0 && !state.startErrorsBySession[action.sessionId]) return state;
      const tasksById = { ...state.tasksById };
      for (const id of ids) delete tasksById[id];
      const taskIdsBySession = { ...state.taskIdsBySession };
      delete taskIdsBySession[action.sessionId];
      const startErrorsBySession = { ...state.startErrorsBySession };
      delete startErrorsBySession[action.sessionId];
      return { ...state, tasksById, taskIdsBySession, startErrorsBySession };
    }

    case "CLEAR_HISTORY":
      return { ...state, history: [] };

    default:
      return state;
  }
}

interface TransferContextValue {
  state: TransferState;
  startTransfer: (
    config: TransferConfig,
    sessionId: string,
    direction: TransferDirection,
    filePaths?: string[],
    downloadDir?: string,
  ) => Promise<TransferStartAck>;
  sendFiles: (sessionId: string, filePaths: string[]) => Promise<void>;
  receiveFiles: (sessionId: string, downloadDir: string) => Promise<void>;
  cancelTransfer: (sessionId: string) => Promise<void>;
  cancelTask: (sessionId: string, transferId: string) => Promise<void>;
  dismissTask: (sessionId: string, transferId: string) => void;
  clearError: (sessionId?: string) => void;
  clearHistory: () => void;
  getTaskForSession: (sessionId: string) => ManagedTransferTask | undefined;
  getActiveTaskForSession: (sessionId: string) => ManagedTransferTask | undefined;
}

const TransferContext = createContext<TransferContextValue | null>(null);

export function TransferProvider({ children }: { children: ReactNode }) {
  const [state, dispatch] = useReducer(transferReducer, initialState);

  const startTransfer = useCallback(
    async (
      config: TransferConfig,
      sessionId: string,
      direction: TransferDirection,
      filePaths?: string[],
      downloadDir?: string,
    ): Promise<TransferStartAck> => {
      dispatch({ type: "TASK_CLEAR_ERROR", sessionId });
      const request: Record<string, unknown> = {
        sessionId,
        protocol: config.protocol,
      };
      if (direction === "send" && filePaths) {
        request.filePaths = filePaths;
      }
      if (direction === "receive") {
        request.remotePaths = [];
        if (downloadDir) request.downloadDir = downloadDir;
      }
      if (config.protocol === "ymodem" && "blockSize" in config) {
        request.blockSize = config.blockSize;
        request.checksumMode = config.checksumMode;
        request.streaming = (config as YmodemTransferConfig).streaming ?? false;
      }

      try {
        const ack = await startFileTransfer(direction, request);
        dispatch({
          type: "TASK_STARTED",
          payload: {
            session_id: sessionId,
            transfer_id: ack.transfer_id,
            protocol: config.protocol,
            direction,
          },
        });
        return ack;
      } catch (error) {
        const message = String(error);
        dispatch({ type: "TASK_START_FAILED", sessionId, error: message });
        throw error;
      }
    },
    [],
  );

  const sendFiles = useCallback(
    async (sessionId: string, filePaths: string[]) => {
      await startTransfer(
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
      await startTransfer(
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
    const current = state.tasksById[transferId];
    if (!current || current.sessionId !== sessionId) {
      throw new Error("Transfer task is no longer active");
    }
    if (isManagedTransferTerminalPhase(current.phase) || current.phase === "cancelling") {
      return;
    }

    const previousPhase = current.phase;
    dispatch({ type: "TASK_CANCELLING", sessionId, transferId });
    try {
      await cancelFileTransfer(sessionId, transferId);
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
  }, [state.tasksById]);

  const cancelTransfer = useCallback(async (sessionId: string) => {
    const task = latestActiveTaskForSession(state, sessionId);
    if (!task) return;
    await cancelTask(sessionId, task.transferId);
  }, [cancelTask, state]);

  const dismissTask = useCallback((sessionId: string, transferId: string) => {
    dispatch({ type: "TASK_DISCARD", sessionId, transferId });
  }, []);

  const clearError = useCallback((sessionId?: string) => {
    dispatch({ type: "TASK_CLEAR_ERROR", sessionId });
  }, []);

  const clearHistory = useCallback(() => {
    dispatch({ type: "CLEAR_HISTORY" });
  }, []);

  const getTaskForSession = useCallback(
    (sessionId: string) => latestTaskForSession(state, sessionId),
    [state],
  );
  const getActiveTaskForSession = useCallback(
    (sessionId: string) => latestActiveTaskForSession(state, sessionId),
    [state],
  );

  useEffect(() => {
    let cancelled = false;
    const unlisteners: UnlistenFn[] = [];

    (async () => {
      const disconnected = await listen<{ session_id: string }>(
        "session-disconnected",
        (event) => {
          dispatch({ type: "TASK_DISCARD_SESSION", sessionId: event.payload.session_id });
        },
      );
      if (cancelled) {
        disconnected();
        return;
      }
      unlisteners.push(disconnected);

      const started = await listen<TransferStartedPayload>(
        "file-transfer:started",
        (event) => dispatch({ type: "TASK_STARTED", payload: event.payload }),
      );
      if (cancelled) {
        started();
        return;
      }
      unlisteners.push(started);

      const progress = await listen<UnifiedTransferProgressPayload>(
        "file-transfer:progress",
        (event) => dispatch({ type: "TASK_PROGRESS", payload: event.payload }),
      );
      if (cancelled) {
        progress();
        return;
      }
      unlisteners.push(progress);

      const finished = await listen<TransferFinishedPayload>(
        "file-transfer:finished",
        (event) => dispatch({ type: "TASK_FINISHED", payload: event.payload }),
      );
      if (cancelled) {
        finished();
        return;
      }
      unlisteners.push(finished);
    })().catch((error) => {
      console.error("TransferContext: Failed to register event listeners:", error);
    });

    return () => {
      cancelled = true;
      unlisteners.forEach((unlisten) => unlisten());
    };
  }, []);

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
        getTaskForSession,
        getActiveTaskForSession,
      }}
    >
      {children}
    </TransferContext.Provider>
  );
}

export function useTransfer() {
  const ctx = useContext(TransferContext);
  if (!ctx) {
    throw new Error("useTransfer must be used within TransferProvider");
  }
  return ctx;
}
