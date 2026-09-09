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
  const { state, cancelTask } = useTransfer();
  const task = state.tasksBySession[sessionId];
  const sftpTask = task?.protocol === "sftp" ? task : undefined;
  const [visible, setVisible] = useState(false);
  const [dismissedTransferId, setDismissedTransferId] = useState<string | null>(null);
  const [localError, setLocalError] = useState<string | null>(null);
  const autoHideTimerRef = useRef<number | null>(null);
  const hoveredRef = useRef(false);

  const clearAutoHideTimer = useCallback(() => {
    if (autoHideTimerRef.current !== null) {
      window.clearTimeout(autoHideTimerRef.current);
      autoHideTimerRef.current = null;
    }
  }, []);

  const scheduleAutoHide = useCallback(() => {
    clearAutoHideTimer();
    autoHideTimerRef.current = window.setTimeout(() => {
      autoHideTimerRef.current = null;
      setVisible(false);
    }, SUCCESS_AUTO_HIDE_MS);
  }, [clearAutoHideTimer]);

  useEffect(() => {
    if (!sftpTask) {
      clearAutoHideTimer();
      setVisible(false);
      return;
    }
    setDismissedTransferId((current) =>
      current === sftpTask.transferId ? current : null,
    );
    setLocalError(null);
    setVisible((current) =>
      dismissedTransferId === sftpTask.transferId ? current : true,
    );
  }, [clearAutoHideTimer, dismissedTransferId, sftpTask?.transferId]);

  useEffect(() => {
    if (!sftpTask || !visible) return;
    clearAutoHideTimer();
    if (sftpTask.phase === "completed" && !hoveredRef.current) {
      scheduleAutoHide();
    }
  }, [clearAutoHideTimer, scheduleAutoHide, sftpTask?.phase, sftpTask?.transferId, visible]);

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
    if (sftpTask) setDismissedTransferId(sftpTask.transferId);
    setVisible(false);
  }, [clearAutoHideTimer, sftpTask]);

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
    if (sftpTask?.phase === "completed") clearAutoHideTimer();
  }, [clearAutoHideTimer, sftpTask?.phase]);

  const resumeAutoHide = useCallback(() => {
    hoveredRef.current = false;
    if (sftpTask?.phase === "completed" && visible) scheduleAutoHide();
  }, [scheduleAutoHide, sftpTask?.phase, visible]);

  return {
    progress,
    hideProgress,
    cancelTransfer,
    pauseAutoHide,
    resumeAutoHide,
  };
}
