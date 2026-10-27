import type { BatchFileEntry, FileTransferState, ProtocolType, TransferDirection, TransferFinishedPayload, TransferHistoryItem, TransferStartedPayload, TransferStatus, UnifiedTransferProgressPayload } from "../types/transfer.ts";
import { PROTOCOL_REGISTRY } from "../types/transfer.ts";

export type ManagedTransferPhase =
  | "preparing"
  | "transferring"
  | "finalizing"
  | "cancelling"
  | "completed"
  | "failed"
  | "cancelled";

export interface ManagedTransferTask {
  runtimeId: string;
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

export type TransferAction =
  | {
      type: "TASK_STARTED" | "TASK_ACKNOWLEDGED";
      payload: TransferStartedPayload;
      initialFiles?: BatchFileEntry[];
    }
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
  | { type: "TASK_DISCARD_SESSION"; sessionId: string; runtimeId: string }
  | { type: "CLEAR_HISTORY" };

export const initialTransferState: TransferState = {
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
  runtimeId: string,
  transferId: string,
  protocol: string,
  direction: TransferDirection,
): ManagedTransferTask {
  return {
    runtimeId,
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
    completedAt: null,
    error: null,
    fileIndex: 0,
    totalFiles: 1,
    aggregateBytes: 0,
    aggregateTotal: 0,
    files: [],
  };
}

function fileNameFromPath(path: string): string {
  const normalized = path.replace(/\\/g, "/").replace(/\/+$/, "");
  return normalized.split("/").pop() || path;
}

export function selectedFileEntries(filePaths?: string[]): BatchFileEntry[] | undefined {
  if (!filePaths || filePaths.length === 0) return undefined;
  return filePaths.map((path) => ({
    fileName: fileNameFromPath(path),
    status: "pending" as const,
    bytesTransferred: 0,
    totalBytes: 0,
  }));
}

function mergeInitialFiles(
  current: BatchFileEntry[],
  initialFiles: BatchFileEntry[] | undefined,
): BatchFileEntry[] {
  if (!initialFiles || initialFiles.length === 0) return current;
  const length = Math.max(current.length, initialFiles.length);
  return Array.from({ length }, (_, index) => current[index] ?? initialFiles[index]);
}

export function latestTaskForSession(
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

export function latestActiveTaskForSession(
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
  if (payload.kind === "batch_complete" || !payload.file_name) {
    return current.files;
  }

  const index = Math.max(0, payload.file_index);
  const next = [...current.files];
  const existing = next[index];
  const status: FileTransferState = payload.kind === "file_complete"
    ? (payload.file_success === false ? "failed" : "completed")
    : "transferring";
  const preserveFailedProgress =
    payload.kind === "file_complete"
    && payload.file_success === false
    && payload.bytes_total <= 0;

  next[index] = {
    fileName: payload.file_name,
    status,
    bytesTransferred: preserveFailedProgress
      ? (existing?.bytesTransferred ?? payload.bytes_done)
      : payload.bytes_done,
    totalBytes: preserveFailedProgress
      ? (existing?.totalBytes ?? payload.bytes_total)
      : payload.bytes_total,
    error: payload.file_error ?? existing?.error,
  };
  return next;
}

function resultProjection(
  payload: TransferFinishedPayload,
  currentFiles: BatchFileEntry[],
): BatchFileEntry[] | null {
  if (!payload.results) return null;

  // Terminal results may include synthesized/skipped entries that never emitted progress.
  // Reconcile byte progress by file identity instead of assuming result indexes match progress indexes.
  const remainingFiles = [...currentFiles];
  return payload.results.map((result) => {
    const existingIndex = remainingFiles.findIndex(
      (entry) => entry.fileName === result.file_name,
    );
    const existing = existingIndex >= 0
      ? remainingFiles.splice(existingIndex, 1)[0]
      : undefined;
    const completed = result.status === "completed";
    const bytesTransferred = completed
      ? result.size
      : (existing?.bytesTransferred ?? 0);
    const totalBytes = existing?.totalBytes && existing.totalBytes > 0
      ? existing.totalBytes
      : Math.max(result.size, bytesTransferred);
    return {
      fileName: result.file_name,
      status: result.status,
      bytesTransferred,
      totalBytes,
      error: result.error ?? existing?.error,
    };
  });
}

export function transferReducer(state: TransferState, action: TransferAction): TransferState {
  switch (action.type) {
    case "TASK_STARTED":
    case "TASK_ACKNOWLEDGED": {
      const payload = action.payload;
      const existing = state.tasksById[payload.transfer_id];
      if (existing) {
        if (existing.runtimeId !== payload.runtime_id) return state;
        const merged = mergeInitialFiles(existing.files, action.initialFiles);
        const files = isManagedTransferTerminalPhase(existing.phase)
          ? merged.map(file => file.status === "pending" || file.status === "transferring" ? { ...file, status: "skipped" as const } : file)
          : merged;
        if (files === existing.files) return state;
        return {
          ...state,
          tasksById: {
            ...state.tasksById,
            [payload.transfer_id]: {
              ...existing,
              fileName: existing.fileName || files[0]?.fileName || "",
              totalFiles: Math.max(existing.totalFiles, files.length),
              files,
            },
          },
        };
      }

      if (action.type === "TASK_ACKNOWLEDGED") return state;
      const oldIds = state.taskIdsBySession[payload.session_id] ?? [];
      const retainedIds = oldIds.filter((id) => {
        const task = state.tasksById[id];
        return task && !isManagedTransferTerminalPhase(task.phase);
      });
      const tasksById = { ...state.tasksById };
      for (const id of oldIds) {
        if (!retainedIds.includes(id)) delete tasksById[id];
      }
      const task = createTask(
        payload.session_id,
        payload.runtime_id,
        payload.transfer_id,
        payload.protocol,
        payload.direction,
      );
      const files = mergeInitialFiles(task.files, action.initialFiles);
      tasksById[payload.transfer_id] = {
        ...task,
        fileName: files[0]?.fileName || task.fileName,
        totalFiles: files.length > 0 ? files.length : task.totalFiles,
        files,
      };
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
      const current = state.tasksById[payload.transfer_id];
      if (!current || current.runtimeId !== payload.runtime_id || isManagedTransferTerminalPhase(current.phase)) return state;
      if (
        current.sessionId !== payload.session_id
        || current.protocol !== payload.protocol
      ) {
        return state;
      }

      const isFileStart = payload.kind === "file_start";
      const isFileComplete = payload.kind === "file_complete";
      const isBatchComplete = payload.kind === "batch_complete";
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
      } else if (isBatchComplete) {
        phase = "finalizing";
      } else if (isFileComplete) {
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
      const nextSpeed = isFileStart ? null : measuredSpeed ?? current.speed;
      const preserveFailedProgress =
        isFileComplete && payload.file_success === false && !knownTotal;

      const nextTask: ManagedTransferTask = {
        ...current,
        direction: payload.direction,
        phase,
        fileName: isBatchComplete ? current.fileName : payload.file_name,
        bytesDone:
          isBatchComplete || preserveFailedProgress
            ? current.bytesDone
            : payload.bytes_done,
        bytesTotal:
          isBatchComplete || preserveFailedProgress
            ? current.bytesTotal
            : payload.bytes_total,
        percent:
          isBatchComplete || preserveFailedProgress
            ? current.percent
            : percent,
        speed: nextSpeed,
        error: payload.file_success === false
          ? (payload.file_error || current.error)
          : current.error,
        fileIndex: isBatchComplete ? current.fileIndex : payload.file_index,
        totalFiles: isBatchComplete
          ? current.totalFiles
          : Math.max(payload.total_files || 0, current.totalFiles),
        aggregateBytes: isBatchComplete
          ? current.aggregateBytes
          : payload.aggregate_bytes,
        aggregateTotal: isBatchComplete
          ? current.aggregateTotal
          : (payload.aggregate_total || current.aggregateTotal),
        files: updateFileProjection(current, payload),
      };

      return {
        ...state,
        tasksById: {
          ...state.tasksById,
          [payload.transfer_id]: nextTask,
        },
        taskIdsBySession: state.taskIdsBySession,
      };
    }

    case "TASK_FINISHED": {
      const payload = action.payload;
      const current = state.tasksById[payload.transfer_id];
      if (current && current.runtimeId !== payload.runtime_id) return state;
      if (!current || current.sessionId !== payload.session_id) return state;
      if (current.protocol !== payload.protocol) return state;
      if (isManagedTransferTerminalPhase(current.phase)) return state;

      const phase: ManagedTransferPhase = payload.success
        ? "completed"
        : payload.cancelled
          ? "cancelled"
          : "failed";
      const error = payload.success ? null : (payload.error || current.error);
      const exactResults = resultProjection(payload, current.files);
      const files = exactResults ?? current.files.map((entry) => {
        if (
          phase !== "completed"
          && (entry.status === "pending" || entry.status === "transferring")
        ) {
          return {
            ...entry,
            status: "skipped" as const,
            error: entry.error ?? error ?? undefined,
          };
        }
        return entry;
      });
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
      const retained = ids.filter(id => state.tasksById[id]?.runtimeId !== action.runtimeId);
      if (retained.length === ids.length) return state;
      const tasksById = { ...state.tasksById };
      for (const id of ids) if (!retained.includes(id)) delete tasksById[id];
      const taskIdsBySession = { ...state.taskIdsBySession };
      if (retained.length) taskIdsBySession[action.sessionId] = retained;
      else delete taskIdsBySession[action.sessionId];
      const startErrorsBySession = { ...state.startErrorsBySession };
      if (!retained.length) delete startErrorsBySession[action.sessionId];
      return { ...state, tasksById, taskIdsBySession, startErrorsBySession };
    }

    case "CLEAR_HISTORY":
      return { ...state, history: [] };

    default:
      return state;
  }
}
