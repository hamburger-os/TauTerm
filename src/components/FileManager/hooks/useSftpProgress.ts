import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  useTransfer,
  type ManagedTransferPhase,
  type ManagedTransferTask,
} from "../../../context/TransferContext";

export type TransferPhase = ManagedTransferPhase;

export interface TransferProgressState {
  visible: boolean;
  transferId: string | null;
  fileName: string;
  direction: "upload" | "download";
  bytesDone: number;
  bytesTotal: number;
  percent: number;
  phase: TransferPhase;
  speed: number | null;
  completedAt: number | null;
  error: string | null;
  fileIndex: number;
  totalFiles: number;
  aggregateBytes: number;
  aggregateTotal: number;
}

const SUCCESS_AUTO_HIDE_MS = 5000;

export function isTransferTerminalPhase(phase: TransferPhase): boolean {
  return phase === "completed" || phase === "failed" || phase === "cancelled";
}

function initialProgressState(): TransferProgressState {
  return {
    visible: false,
    transferId: null,
    fileName: "",
    direction: "download",
    bytesDone: 0,
    bytesTotal: 0,
    percent: 0,
    phase: "completed",
    speed: null,
    completedAt: null,
    error: null,
    fileIndex: 0,
    totalFiles: 1,
    aggregateBytes: 0,
    aggregateTotal: 0,
  };
}

function displayFileName(rawName: string): string {
  if (!rawName || rawName === "__batch_complete__") return "";
  if (rawName.includes("/")) return rawName.split("/").pop() || rawName;
  if (rawName.includes("\\")) return rawName.split("\\").pop() || rawName;
  return rawName;
}

function toProgress(task: ManagedTransferTask, error: string | null): TransferProgressState {
  return {
    visible: true,
    transferId: task.transferId,
    fileName: displayFileName(task.fileName),
    direction: task.direction === "send" ? "upload" : "download",
    bytesDone: task.bytesDone,
    bytesTotal: task.bytesTotal,
    percent: task.percent,
    phase: task.phase,
    speed: task.speed,
    completedAt: task.completedAt,
    error: error ?? task.error,
    fileIndex: task.fileIndex,
    totalFiles: task.totalFiles,
    aggregateBytes: task.aggregateBytes,
    aggregateTotal: task.aggregateTotal,
  };
}

/**
 * 文件管理器的紧凑传输投影。
 *
 * 后端 started/progress/finished 只由顶层 TransferContext 监听一次；这里不再拥有
 * 第二套 transfer_id/终态状态机，只负责可见性、成功自动收起和悬停暂停。
 */
export function useSftpProgress(sessionId: string) {
  const { state, cancelTask, dismissTask } = useTransfer();
  const task = state.tasksBySession[sessionId];
  const sftpTask = task?.protocol === "sftp" ? task : undefined;
  const [visible, setVisible] = useState(false);
  const [dismissedTransferId, setDismissedTransferId] = useState<string | null>(null);
  const [localError, setLocalError] = useState<string | null>(null);
  const autoHideTimerRef = useRef<number | null>(null);
  const autoHideDeadlineRef = useRef<number | null>(null);
  const autoHideRemainingRef = useRef(SUCCESS_AUTO_HIDE_MS);
  const autoHideTransferIdRef = useRef<string | null>(null);
  const hoveredRef = useRef(false);

  const clearAutoHideTimer = useCallback(() => {
    if (autoHideTimerRef.current !== null) {
      window.clearTimeout(autoHideTimerRef.current);
      autoHideTimerRef.current = null;
    }
  }, []);

  const scheduleAutoHide = useCallback((
    transferId: string,
    delayMs = SUCCESS_AUTO_HIDE_MS,
  ) => {
    clearAutoHideTimer();
    const delay = Math.max(0, delayMs);
    autoHideRemainingRef.current = delay;
    autoHideDeadlineRef.current = Date.now() + delay;
    autoHideTimerRef.current = window.setTimeout(() => {
      autoHideTimerRef.current = null;
      autoHideDeadlineRef.current = null;
      autoHideRemainingRef.current = 0;
      setDismissedTransferId(transferId);
      setVisible(false);
      // Successful cards are not only hidden locally: remove the exact finished
      // snapshot so reopening/remounting the right sidebar cannot resurrect it.
      dismissTask(sessionId, transferId);
    }, delay);
  }, [clearAutoHideTimer, dismissTask, sessionId]);

  useEffect(() => {
    if (!sftpTask) {
      clearAutoHideTimer();
      autoHideDeadlineRef.current = null;
      autoHideRemainingRef.current = SUCCESS_AUTO_HIDE_MS;
      autoHideTransferIdRef.current = null;
      hoveredRef.current = false;
      setVisible(false);
      return;
    }

    if (autoHideTransferIdRef.current !== sftpTask.transferId) {
      clearAutoHideTimer();
      autoHideTransferIdRef.current = sftpTask.transferId;
      autoHideDeadlineRef.current = null;
      autoHideRemainingRef.current = SUCCESS_AUTO_HIDE_MS;
      // A previous card can disappear while hovered (for example by pressing its
      // close button), in which case the browser need not dispatch mouseleave.
      // Do not let that stale hover state suppress auto-hide for the next task.
      hoveredRef.current = false;
    }

    if (
      sftpTask.phase === "completed"
      && sftpTask.completedAt !== null
      && Date.now() - sftpTask.completedAt >= SUCCESS_AUTO_HIDE_MS
    ) {
      setDismissedTransferId(sftpTask.transferId);
      setVisible(false);
      dismissTask(sessionId, sftpTask.transferId);
      return;
    }

    setDismissedTransferId((current) =>
      current === sftpTask.transferId ? current : null,
    );
    setLocalError(null);
    setVisible((current) =>
      dismissedTransferId === sftpTask.transferId ? current : true,
    );
  }, [
    clearAutoHideTimer,
    dismissTask,
    dismissedTransferId,
    sessionId,
    sftpTask?.completedAt,
    sftpTask?.phase,
    sftpTask?.transferId,
  ]);

  useEffect(() => {
    if (!sftpTask || !visible) return;
    clearAutoHideTimer();
    if (sftpTask.phase === "completed" && !hoveredRef.current) {
      const elapsed = sftpTask.completedAt === null
        ? 0
        : Math.max(0, Date.now() - sftpTask.completedAt);
      scheduleAutoHide(
        sftpTask.transferId,
        Math.max(0, SUCCESS_AUTO_HIDE_MS - elapsed),
      );
    }
  }, [
    clearAutoHideTimer,
    scheduleAutoHide,
    sftpTask?.completedAt,
    sftpTask?.phase,
    sftpTask?.transferId,
    visible,
  ]);

  useEffect(() => () => clearAutoHideTimer(), [clearAutoHideTimer]);

  const progress = useMemo(() => {
    if (
      !sftpTask
      || !visible
      || dismissedTransferId === sftpTask.transferId
    ) {
      return initialProgressState();
    }
    return toProgress(sftpTask, localError);
  }, [dismissedTransferId, localError, sftpTask, visible]);

  const hideProgress = useCallback(() => {
    clearAutoHideTimer();
    hoveredRef.current = false;
    autoHideDeadlineRef.current = null;
    autoHideRemainingRef.current = SUCCESS_AUTO_HIDE_MS;
    if (sftpTask) {
      setDismissedTransferId(sftpTask.transferId);
      dismissTask(sessionId, sftpTask.transferId);
    }
    setVisible(false);
  }, [clearAutoHideTimer, dismissTask, sessionId, sftpTask]);

  const cancelTransfer = useCallback(async () => {
    if (!sftpTask || isTransferTerminalPhase(sftpTask.phase) || sftpTask.phase === "cancelling") {
      return;
    }
    clearAutoHideTimer();
    setLocalError(null);
    try {
      await cancelTask(sessionId, sftpTask.transferId);
    } catch (error) {
      setLocalError(String(error));
    }
  }, [cancelTask, clearAutoHideTimer, sessionId, sftpTask]);

  const pauseAutoHide = useCallback(() => {
    hoveredRef.current = true;
    if (sftpTask?.phase !== "completed") return;

    if (autoHideDeadlineRef.current !== null) {
      autoHideRemainingRef.current = Math.max(
        0,
        autoHideDeadlineRef.current - Date.now(),
      );
      autoHideDeadlineRef.current = null;
    }
    clearAutoHideTimer();
  }, [clearAutoHideTimer, sftpTask?.phase]);

  const resumeAutoHide = useCallback(() => {
    hoveredRef.current = false;
    if (sftpTask?.phase === "completed" && visible) {
      scheduleAutoHide(sftpTask.transferId, autoHideRemainingRef.current);
    }
  }, [scheduleAutoHide, sftpTask?.phase, sftpTask?.transferId, visible]);

  return {
    progress,
    hideProgress,
    cancelTransfer,
    pauseAutoHide,
    resumeAutoHide,
  };
}
