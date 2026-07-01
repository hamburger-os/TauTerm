import { useRef, useCallback, useState, useEffect } from "react";
import type { CommandItem } from "./types";

interface UseCommandRunnerOptions {
  onSend: (cmd: CommandItem) => Promise<void>;
}

interface UseCommandRunner {
  isRunning: boolean;
  currentIndex: number | null;
  /** Loop progress. null when not running. current=0-based, total=-1 for infinite */
  loopProgress: { current: number; total: number } | null;
  start: (commands: CommandItem[], loopCount: number) => void;
  stop: () => void;
}

/** 延迟函数 */
function sleep(ms: number): { promise: Promise<void>; cancel: () => void } {
  let timer: ReturnType<typeof setTimeout> | null = null;
  let rejectFn: ((reason?: unknown) => void) | null = null;
  const promise = new Promise<void>((resolve, reject) => {
    rejectFn = reject;
    timer = setTimeout(resolve, ms);
  });
  return {
    promise,
    cancel: () => {
      if (timer) { clearTimeout(timer); timer = null; }
      if (rejectFn) rejectFn(new Error("CANCELLED"));
    },
  };
}

/**
 * 命令执行引擎 Hook
 *
 * 管理命令队列的逐条执行，支持单次/循环模式，
 * 每条命令独立延时，支持中途停止。
 */
export default function useCommandRunner({ onSend }: UseCommandRunnerOptions): UseCommandRunner {
  const [isRunning, setIsRunning] = useState(false);
  const [currentIndex, setCurrentIndex] = useState<number | null>(null);
  const [loopProgress, setLoopProgress] = useState<{ current: number; total: number } | null>(null);

  const stopFlagRef = useRef(false);
  const cancelSleepRef = useRef<(() => void) | null>(null);

  // 组件卸载时清理
  useEffect(() => {
    return () => {
      stopFlagRef.current = true;
      cancelSleepRef.current?.();
      setIsRunning(false);
      setCurrentIndex(null);
      setLoopProgress(null);
    };
  }, []);

  const stop = useCallback(() => {
    stopFlagRef.current = true;
    cancelSleepRef.current?.();
    setIsRunning(false);
    setCurrentIndex(null);
    setLoopProgress(null);
  }, []);

  const start = useCallback(async (commands: CommandItem[], loopCount: number) => {
    if (commands.length === 0) return;

    stopFlagRef.current = false;
    setIsRunning(true);
    const maxLoops = loopCount === 0 ? Infinity : loopCount;

    try {
      let loopIndex = 0;
      while (loopIndex < maxLoops) {
        for (let i = 0; i < commands.length; i++) {
          if (stopFlagRef.current) break;
          setCurrentIndex(i);
          setLoopProgress({ current: loopIndex, total: loopCount === 0 ? -1 : loopCount });
          await onSend(commands[i]);
          if (stopFlagRef.current) break;

          // Apply delay (skip last command of last loop)
          const isLastCmdOfLastLoop = loopIndex === maxLoops - 1 && i === commands.length - 1;
          const delay = Math.max(0, commands[i].delay);
          if (delay > 0 && !isLastCmdOfLastLoop) {
            const s = sleep(delay);
            cancelSleepRef.current = s.cancel;
            try { await s.promise; } catch { break; }
          }
        }
        if (stopFlagRef.current) break;
        loopIndex++;
      }
    } catch {
      // 发送失败时停止
    } finally {
      setIsRunning(false);
      setCurrentIndex(null);
      setLoopProgress(null);
    }
  }, [onSend]);

  return { isRunning, currentIndex, loopProgress, start, stop };
}
