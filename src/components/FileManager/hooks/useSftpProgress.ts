import { useState, useEffect, useCallback, useRef } from 'react';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import type { UnifiedProgressPayload, TransferFinishedPayload } from '../types';

export interface TransferProgressState {
  visible: boolean;
  fileName: string;
  direction: 'upload' | 'download';
  bytesDone: number;
  bytesTotal: number;
  percent: number;
  startTime: number;
  finished: boolean;
  speed: number;
  /** 批次进度 — 当前文件索引 (0-based) */
  fileIndex: number;
  /** 批次进度 — 文件总数 */
  totalFiles: number;
  /** 批次进度 — 聚合已传输字节 */
  aggregateBytes: number;
  /** 批次进度 — 聚合总字节 */
  aggregateTotal: number;
}

/**
 * 统一文件传输进度 Hook（无自动隐藏）
 *
 * 监听 `file-transfer:progress` / `file-transfer:started` / `file-transfer:finished`
 * 事件，管理进度条显示状态。进度条在完成后保持显示，由用户手动点×关闭。
 *
 * `TransferProgressBar` 为纯视觉组件，不包含任何自动隐藏逻辑。
 */
export function useSftpProgress(sessionId: string) {
  const [progress, setProgress] = useState<TransferProgressState>({
    visible: false,
    fileName: '',
    direction: 'download',
    bytesDone: 0,
    bytesTotal: 0,
    percent: 0,
    startTime: 0,
    finished: true,
    speed: 0,
    fileIndex: 0,
    totalFiles: 1,
    aggregateBytes: 0,
    aggregateTotal: 0,
  });
  const lastSampleRef = useRef<{ bytesDone: number; time: number } | null>(null);
  const speedRef = useRef(0);

  const resetProgressState = useCallback(() => {
    setProgress({
      visible: false, fileName: '', direction: 'download',
      bytesDone: 0, bytesTotal: 0, percent: 0, startTime: 0,
      finished: true, speed: 0,
      fileIndex: 0, totalFiles: 1, aggregateBytes: 0, aggregateTotal: 0,
    });
    lastSampleRef.current = null;
    speedRef.current = 0;
  }, []);

  useEffect(() => {
    let unlistenProgress: UnlistenFn | undefined;
    let unlistenStarted: UnlistenFn | undefined;
    let unlistenFinished: UnlistenFn | undefined;

    // 监听传输启动事件，立即显示进度条（消除点击到首次进度事件之间的静默期）
    listen<{ session_id: string; protocol: string; direction: string }>(
      'file-transfer:started',
      (event) => {
        if (event.payload.protocol !== 'sftp') return;
        if (event.payload.session_id !== sessionId) return;
        const now = Date.now();
        setProgress(prev => ({
          ...prev,
          visible: true,
          fileName: 'Preparing...',
          direction: event.payload.direction === 'send' ? 'upload' : 'download',
          percent: 0,
          finished: false,
          startTime: now,
        }));
      },
    ).then(fn => { unlistenStarted = fn; });

    listen<UnifiedProgressPayload>('file-transfer:progress', (event) => {
      if (event.payload.protocol !== 'sftp') return; // 仅处理 SFTP
      if (event.payload.session_id !== sessionId) return; // 仅处理当前会话
      const now = Date.now();
      const percent = event.payload.bytes_total > 0
        ? Math.round((event.payload.bytes_done / event.payload.bytes_total) * 100)
        : 0;

      // 跨文件边界不重置 lastSampleRef：
      // bytes_done 回退→db<0→进入 else 分支保留上一速度，平稳过渡不归零

      let speed = speedRef.current;
      const prevSample = lastSampleRef.current;
      if (prevSample) {
        const dt = (now - prevSample.time) / 1000;
        const db = event.payload.bytes_done - prevSample.bytesDone;
        if (dt > 0 && db >= 0) {
          const instant = db / dt;
          // 时间加权 EMA（τ=0.5s，约 1.15s 达到 90% 真实值，自适应采样频率）
          const alpha = 1 - Math.exp(-dt / 0.5);
          speed = speedRef.current > 0
            ? instant * alpha + speedRef.current * (1 - alpha)
            : instant * 0.5; // 首个有效样本使用 50% 权重
        } else {
          speed = speedRef.current;
        }
      }
      lastSampleRef.current = { bytesDone: event.payload.bytes_done, time: now };
      speedRef.current = speed;

      // 防御性提取纯文件名（避免完整路径被显示）
      // 仅当 file_name 非空时更新（batch_complete 事件 file_name 为空，保留当前显示）
      const rawName = event.payload.file_name;
      const displayName = rawName
        ? (rawName.includes('/')
            ? (rawName.split('/').pop() || rawName)
            : (rawName.includes('\\') ? (rawName.split('\\').pop() || rawName) : rawName))
        : undefined;

      // 批次完成时保留当前单文件进度字段，避免 batch_complete 的零值覆盖
      const isBatchComplete = event.payload.is_batch_complete;

      setProgress(prev => ({
        visible: true,
        fileName: isBatchComplete ? prev.fileName : (displayName ?? prev.fileName),
        direction: event.payload.direction === 'send' ? 'upload' : 'download',
        bytesDone: isBatchComplete ? prev.bytesDone : event.payload.bytes_done,
        bytesTotal: isBatchComplete ? prev.bytesTotal : event.payload.bytes_total,
        percent: isBatchComplete ? prev.percent : percent,
        startTime: prev.startTime || now,
        finished: prev.finished,
        speed: speed || prev.speed,
        fileIndex: isBatchComplete ? prev.fileIndex : (event.payload.file_index ?? prev.fileIndex),
        totalFiles: isBatchComplete ? prev.totalFiles : (event.payload.total_files || prev.totalFiles),
        aggregateBytes: isBatchComplete ? prev.aggregateBytes : (event.payload.aggregate_bytes ?? prev.aggregateBytes),
        aggregateTotal: isBatchComplete ? prev.aggregateTotal : (event.payload.aggregate_total || prev.aggregateTotal),
      }));
    }).then(fn => { unlistenProgress = fn; });

    listen<TransferFinishedPayload>('file-transfer:finished', (event) => {
      if (event.payload.protocol !== 'sftp') return;
      if (event.payload.session_id !== sessionId) return;
      const ok = event.payload.success;
      setProgress(prev => ({ ...prev, finished: true, percent: ok ? 100 : prev.percent }));
    }).then(fn => { unlistenFinished = fn; });

    return () => {
      if (unlistenProgress) unlistenProgress();
      if (unlistenStarted) unlistenStarted();
      if (unlistenFinished) unlistenFinished();
    };
  }, [sessionId, resetProgressState]);

  const hideProgress = useCallback(() => {
    resetProgressState();
  }, [resetProgressState]);

  const cancelTransfer = useCallback(async () => {
    try {
      await invoke('file_transfer_cancel', { sessionId });
    } catch (e) {
      console.error('取消传输失败:', e);
    }
    setProgress(prev => ({ ...prev, finished: true }));
  }, [sessionId, resetProgressState]);

  return { progress, hideProgress, cancelTransfer };
}
