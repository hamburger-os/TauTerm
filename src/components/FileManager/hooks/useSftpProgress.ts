import { useState, useEffect, useCallback, useRef } from 'react';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import type {
  TransferFinishedPayload,
  TransferStartedPayload,
  UnifiedProgressPayload,
} from '../types';

export type TransferPhase =
  | 'preparing'
  | 'transferring'
  | 'finalizing'
  | 'cancelling'
  | 'completed'
  | 'failed'
  | 'cancelled';

export interface TransferProgressState {
  visible: boolean;
  transferId: string | null;
  fileName: string;
  direction: 'upload' | 'download';
  bytesDone: number;
  bytesTotal: number;
  percent: number;
  phase: TransferPhase;
  /** 后端真实 SFTP I/O 层测得的速率；null 表示当前没有可靠样本。 */
  speed: number | null;
  error: string | null;
  /** 批次进度 — 当前文件索引 (0-based) */
  fileIndex: number;
  /** 批次进度 — 文件总数 */
  totalFiles: number;
  /** 批次进度 — 聚合已传输字节 */
  aggregateBytes: number;
  /** 批次进度 — 聚合总字节 */
  aggregateTotal: number;
}

const SUCCESS_AUTO_HIDE_MS = 5000;

export function isTransferTerminalPhase(phase: TransferPhase): boolean {
  return phase === 'completed' || phase === 'failed' || phase === 'cancelled';
}

function initialProgressState(): TransferProgressState {
  return {
    visible: false,
    transferId: null,
    fileName: '',
    direction: 'download',
    bytesDone: 0,
    bytesTotal: 0,
    percent: 0,
    phase: 'completed',
    speed: null,
    error: null,
    fileIndex: 0,
    totalFiles: 1,
    aggregateBytes: 0,
    aggregateTotal: 0,
  };
}

function displayFileName(rawName: string): string {
  if (!rawName || rawName === '__batch_complete__') return '';
  if (rawName.includes('/')) return rawName.split('/').pop() || rawName;
  if (rawName.includes('\\')) return rawName.split('\\').pop() || rawName;
  return rawName;
}

/**
 * SFTP 传输状态机。
 *
 * 事件顺序由后端保证：started → progress* → broadcaster drain → finished。
 * 每个事件携带 transfer_id，迟到的旧传输事件不会污染下一次传输。
 * 成功状态 5 秒后自动收起；悬停会暂停计时；失败/取消保持到用户关闭。
 */
export function useSftpProgress(sessionId: string) {
  const [progress, setProgress] = useState<TransferProgressState>(initialProgressState);
  const activeTransferIdRef = useRef<string | null>(null);
  const autoHideTimerRef = useRef<number | null>(null);
  const phaseRef = useRef<TransferPhase>('completed');
  const hoveredRef = useRef(false);

  const clearAutoHideTimer = useCallback(() => {
    if (autoHideTimerRef.current !== null) {
      window.clearTimeout(autoHideTimerRef.current);
      autoHideTimerRef.current = null;
    }
  }, []);

  const resetProgressState = useCallback(() => {
    clearAutoHideTimer();
    activeTransferIdRef.current = null;
    phaseRef.current = 'completed';
    setProgress(initialProgressState());
  }, [clearAutoHideTimer]);

  const scheduleAutoHide = useCallback(() => {
    clearAutoHideTimer();
    autoHideTimerRef.current = window.setTimeout(() => {
      autoHideTimerRef.current = null;
      activeTransferIdRef.current = null;
      phaseRef.current = 'completed';
      setProgress(initialProgressState());
    }, SUCCESS_AUTO_HIDE_MS);
  }, [clearAutoHideTimer]);

  const pauseAutoHide = useCallback(() => {
    hoveredRef.current = true;
    if (phaseRef.current === 'completed') clearAutoHideTimer();
  }, [clearAutoHideTimer]);

  const resumeAutoHide = useCallback(() => {
    hoveredRef.current = false;
    if (phaseRef.current === 'completed') scheduleAutoHide();
  }, [scheduleAutoHide]);

  useEffect(() => {
    let unlistenProgress: UnlistenFn | undefined;
    let unlistenStarted: UnlistenFn | undefined;
    let unlistenFinished: UnlistenFn | undefined;

    listen<TransferStartedPayload>('file-transfer:started', (event) => {
      const payload = event.payload;
      if (payload.protocol !== 'sftp' || payload.session_id !== sessionId) return;

      clearAutoHideTimer();
      activeTransferIdRef.current = payload.transfer_id;
      phaseRef.current = 'preparing';
      setProgress({
        ...initialProgressState(),
        visible: true,
        transferId: payload.transfer_id,
        direction: payload.direction === 'send' ? 'upload' : 'download',
        phase: 'preparing',
      });
    }).then(fn => { unlistenStarted = fn; });

    listen<UnifiedProgressPayload>('file-transfer:progress', (event) => {
      const payload = event.payload;
      if (payload.protocol !== 'sftp' || payload.session_id !== sessionId) return;
      if (!payload.transfer_id || payload.transfer_id !== activeTransferIdRef.current) return;

      const name = displayFileName(payload.file_name);
      const isBatchComplete = payload.is_batch_complete;
      const hasKnownTotal = payload.bytes_total > 0;
      const percent = hasKnownTotal
        ? payload.bytes_done >= payload.bytes_total
          ? 100
          : Math.min(99, Math.max(0, Math.floor((payload.bytes_done / payload.bytes_total) * 100)))
        : 0;
      const backendSpeed =
        typeof payload.bytes_per_second === 'number'
        && Number.isFinite(payload.bytes_per_second)
        && payload.bytes_per_second > 0
          ? payload.bytes_per_second
          : null;

      setProgress(prev => {
        if (prev.transferId !== payload.transfer_id || isTransferTerminalPhase(prev.phase)) {
          return prev;
        }

        let phase: TransferPhase = prev.phase;
        let error = prev.error;
        let speed = prev.speed;

        if (payload.is_file_start) {
          phase = 'transferring';
          speed = null;
          error = null;
        } else if (isBatchComplete) {
          phase = 'finalizing';
          speed = null;
          if (payload.file_success === false && !error) {
            error = payload.file_error || null;
          }
        } else if (payload.is_file_complete) {
          phase = 'finalizing';
          speed = null;
          if (payload.file_success === false) {
            error = payload.file_error || error;
          }
        } else {
          phase = hasKnownTotal && payload.bytes_done >= payload.bytes_total
            ? 'finalizing'
            : 'transferring';
          speed = phase === 'transferring' ? (backendSpeed ?? prev.speed) : null;
        }

        phaseRef.current = phase;
        return {
          ...prev,
          visible: true,
          fileName: isBatchComplete ? prev.fileName : (name || prev.fileName),
          direction: payload.direction === 'send' ? 'upload' : 'download',
          bytesDone: isBatchComplete ? prev.bytesDone : payload.bytes_done,
          bytesTotal: isBatchComplete ? prev.bytesTotal : payload.bytes_total,
          percent: isBatchComplete ? prev.percent : percent,
          phase,
          speed,
          error,
          fileIndex: isBatchComplete ? prev.fileIndex : payload.file_index,
          totalFiles: isBatchComplete ? prev.totalFiles : (payload.total_files || prev.totalFiles),
          aggregateBytes: isBatchComplete ? prev.aggregateBytes : payload.aggregate_bytes,
          aggregateTotal: isBatchComplete
            ? prev.aggregateTotal
            : (payload.aggregate_total || prev.aggregateTotal),
        };
      });
    }).then(fn => { unlistenProgress = fn; });

    listen<TransferFinishedPayload>('file-transfer:finished', (event) => {
      const payload = event.payload;
      if (payload.session_id !== sessionId) return;
      if (payload.protocol && payload.protocol !== 'sftp') return;
      if (
        !payload.transfer_id
        || !activeTransferIdRef.current
        || payload.transfer_id !== activeTransferIdRef.current
      ) {
        return;
      }

      clearAutoHideTimer();
      const phase: TransferPhase = payload.success
        ? 'completed'
        : payload.cancelled
          ? 'cancelled'
          : 'failed';
      phaseRef.current = phase;

      setProgress(prev => {
        if (
          payload.transfer_id
          && prev.transferId
          && payload.transfer_id !== prev.transferId
        ) {
          return prev;
        }
        return {
          ...prev,
          visible: true,
          transferId: payload.transfer_id ?? prev.transferId,
          phase,
          percent: payload.success ? 100 : prev.percent,
          speed: null,
          error: payload.success ? null : (payload.error || prev.error),
        };
      });

      if (payload.success && !hoveredRef.current) scheduleAutoHide();
    }).then(fn => { unlistenFinished = fn; });

    return () => {
      clearAutoHideTimer();
      if (unlistenProgress) unlistenProgress();
      if (unlistenStarted) unlistenStarted();
      if (unlistenFinished) unlistenFinished();
    };
  }, [clearAutoHideTimer, scheduleAutoHide, sessionId]);

  const hideProgress = useCallback(() => {
    resetProgressState();
  }, [resetProgressState]);

  const cancelTransfer = useCallback(async () => {
    if (!activeTransferIdRef.current || isTransferTerminalPhase(phaseRef.current)) return;

    clearAutoHideTimer();
    const previousPhase = phaseRef.current;
    phaseRef.current = 'cancelling';
    setProgress(prev => ({ ...prev, phase: 'cancelling', speed: null }));

    try {
      await invoke('file_transfer_cancel', { sessionId });
    } catch (error) {
      // 取消命令失败不等于传输失败；恢复原运行状态，最终结果仍由 finished 决定。
      phaseRef.current = previousPhase;
      setProgress(prev => ({
        ...prev,
        phase: previousPhase,
        error: String(error),
      }));
    }
  }, [clearAutoHideTimer, sessionId]);

  return {
    progress,
    hideProgress,
    cancelTransfer,
    pauseAutoHide,
    resumeAutoHide,
  };
}
