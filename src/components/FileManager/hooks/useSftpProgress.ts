import { useState, useEffect, useCallback, useRef } from 'react';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { SftpProgressPayload } from '../types';

export interface TransferProgressState {
  visible: boolean;
  fileName: string;
  direction: 'upload' | 'download';
  bytesDone: number;
  bytesTotal: number;
  percent: number;
  startTime: number;
  /** 传输已完成（含取消/失败）—— 用于区分 × 按钮是"关闭已完成提示"还是"中断进行中传输" */
  finished: boolean;
  /** EMA 平滑后的瞬时速度（字节/秒），由进度事件增量计算得出，避免全程平均速度失真 */
  speed: number;
}

/** SFTP 传输完成事件载荷（对应后端 `sftp-transfer-finished` emit） */
interface SftpTransferFinishedPayload {
  session_id: string;
  direction: 'upload' | 'download';
  file_name: string;
  result: { bytes?: number; error?: string };
}

/** 取消/失败后延迟自动隐藏进度条的时间（ms），给用户"已停止"的视觉反馈 */
const AUTO_HIDE_DELAY_MS = 1500;

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
  });
  /** 最近一次传输是否成功（供 useFileManager 决定是否刷新目录） */
  const [lastTransferOk, setLastTransferOk] = useState<boolean | null>(null);

  // 速度计算所需的上次采样点（不放入 state 避免重渲染）
  const lastSampleRef = useRef<{ bytesDone: number; time: number } | null>(null);
  // 自动隐藏定时器引用
  const autoHideTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  // 当前速度的 ref 镜像（供事件监听器读取最新值，避免将 progress.speed 放入 effect 依赖）
  const speedRef = useRef(0);

  /** 重置进度状态到初始值（隐藏时调用，确保下次传输速度计算正确） */
  const resetProgressState = useCallback(() => {
    setProgress({
      visible: false,
      fileName: '',
      direction: 'download',
      bytesDone: 0,
      bytesTotal: 0,
      percent: 0,
      startTime: 0,
      finished: true,
      speed: 0,
    });
    lastSampleRef.current = null;
    speedRef.current = 0;
  }, []);

  useEffect(() => {
    let unlistenProgress: UnlistenFn | undefined;
    let unlistenFinished: UnlistenFn | undefined;

    listen<SftpProgressPayload>('sftp-progress', (event) => {
      if (event.payload.session_id === sessionId) {
        const now = Date.now();
        const percent =
          event.payload.bytes_total > 0
            ? Math.round(
                (event.payload.bytes_done / event.payload.bytes_total) * 100
              )
            : 0;

        // 基于本次与上次采样的增量计算瞬时速度，再用 EMA 平滑
        let speed = 0;
        const prevSample = lastSampleRef.current;
        if (prevSample) {
          const dt = (now - prevSample.time) / 1000;
          const db = event.payload.bytes_done - prevSample.bytesDone;
          if (dt > 0 && db >= 0) {
            const instant = db / dt;
            speed = prevSample.bytesDone > 0 ? instant * 0.4 + speedRef.current * 0.6 : instant;
          } else {
            speed = speedRef.current;
          }
        }
        lastSampleRef.current = { bytesDone: event.payload.bytes_done, time: now };
        speedRef.current = speed;

        // 取消任何挂起的自动隐藏（新进度事件到达说明传输仍在进行）
        if (autoHideTimerRef.current) {
          clearTimeout(autoHideTimerRef.current);
          autoHideTimerRef.current = null;
        }

        setProgress(prev => ({
          visible: true,
          fileName: event.payload.file_name,
          direction: event.payload.direction,
          bytesDone: event.payload.bytes_done,
          bytesTotal: event.payload.bytes_total,
          percent,
          startTime: prev.startTime || now,
          finished: percent >= 100,
          speed: speed || prev.speed,
        }));
      }
    }).then(fn => {
      unlistenProgress = fn;
    });

    // 监听传输完成事件：后端在后台线程结束时 emit。
    // 由于传输命令已改为非阻塞（spawn 后立即返回），前端不能依赖 invoke 的 Promise
    // 来获知传输结束，必须通过此事件。
    listen<SftpTransferFinishedPayload>('sftp-transfer-finished', (event) => {
      if (event.payload.session_id === sessionId) {
        const ok = !event.payload.result.error;
        setLastTransferOk(ok);
        setProgress(prev => ({
          ...prev,
          finished: true,
          percent: ok ? 100 : prev.percent,
        }));
        // 失败/取消时延迟自动隐藏，让用户看到"已停止"状态后再淡出
        if (!ok) {
          if (autoHideTimerRef.current) clearTimeout(autoHideTimerRef.current);
          autoHideTimerRef.current = setTimeout(() => {
            resetProgressState();
            autoHideTimerRef.current = null;
          }, AUTO_HIDE_DELAY_MS);
        }
      }
    }).then(fn => {
      unlistenFinished = fn;
    });

    return () => {
      if (unlistenProgress) unlistenProgress();
      if (unlistenFinished) unlistenFinished();
      if (autoHideTimerRef.current) {
        clearTimeout(autoHideTimerRef.current);
        autoHideTimerRef.current = null;
      }
    };
  }, [sessionId, resetProgressState]);

  const hideProgress = useCallback(() => {
    // 重置所有状态（含 startTime 和 speed），确保下次传输时速度计算从头开始
    if (autoHideTimerRef.current) {
      clearTimeout(autoHideTimerRef.current);
      autoHideTimerRef.current = null;
    }
    resetProgressState();
  }, [resetProgressState]);

  /** 中断当前 SFTP 传输（调用后端取消命令，传输循环在下次块检查时退出） */
  const cancelTransfer = useCallback(async () => {
    try {
      await invoke('cancel_sftp_transfer', { sessionId });
    } catch (e) {
      console.error('取消 SFTP 传输失败:', e);
    }
    // 标记为已完成：后端传输函数将返回错误，不会再发进度事件。
    // 不立即隐藏进度条，给用户"已停止"的视觉反馈，1.5s 后自动隐藏。
    setProgress(prev => ({ ...prev, finished: true }));
    setLastTransferOk(false);
    if (autoHideTimerRef.current) clearTimeout(autoHideTimerRef.current);
    autoHideTimerRef.current = setTimeout(() => {
      resetProgressState();
      autoHideTimerRef.current = null;
    }, AUTO_HIDE_DELAY_MS);
  }, [sessionId, resetProgressState]);

  return { progress, hideProgress, cancelTransfer, lastTransferOk };
}
