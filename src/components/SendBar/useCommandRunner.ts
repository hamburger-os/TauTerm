import { useCallback, useEffect, useRef, useState } from "react";
import type { CommandItem } from "./types";

interface UseCommandRunnerOptions {
  onSend: (cmd: CommandItem) => Promise<void>;
}

interface UseCommandRunner {
  isRunning: boolean;
  currentIndex: number | null;
  /** Loop progress. null when not running. current=0-based, total=-1 for infinite. */
  loopProgress: { current: number; total: number } | null;
  start: (commands: CommandItem[], loopCount: number) => void;
  stop: () => void;
}

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
      if (timer) {
        clearTimeout(timer);
        timer = null;
      }
      if (rejectFn) {
        rejectFn(new Error("CANCELLED"));
        rejectFn = null;
      }
    },
  };
}

/**
 * 串行命令执行器。内部立即用 ref 抢占运行权，避免快速双击在 React state 刷新前
 * 启动两条并发执行链。停止时先取消后续工作；若当前底层 send 尚未返回，则保持
 * running 状态直到该调用真正结束，避免用户立即重启形成两条发送链。
 */
export default function useCommandRunner({ onSend }: UseCommandRunnerOptions): UseCommandRunner {
  const [isRunning, setIsRunning] = useState(false);
  const [currentIndex, setCurrentIndex] = useState<number | null>(null);
  const [loopProgress, setLoopProgress] = useState<{ current: number; total: number } | null>(null);

  const mountedRef = useRef(true);
  const runningRef = useRef(false);
  const stopFlagRef = useRef(false);
  const cancelSleepRef = useRef<(() => void) | null>(null);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      stopFlagRef.current = true;
      cancelSleepRef.current?.();
      cancelSleepRef.current = null;
    };
  }, []);

  const stop = useCallback(() => {
    stopFlagRef.current = true;
    cancelSleepRef.current?.();
    cancelSleepRef.current = null;
    // Do not clear runningRef/isRunning here. A send already in flight cannot be cancelled by this
    // hook, so the execution remains locked until the async chain reaches finally.
  }, []);

  const start = useCallback(async (commands: CommandItem[], loopCount: number) => {
    if (commands.length === 0 || runningRef.current) return;

    runningRef.current = true;
    stopFlagRef.current = false;
    if (mountedRef.current) setIsRunning(true);
    const maxLoops = loopCount === 0 ? Infinity : Math.max(1, loopCount);

    try {
      let loopIndex = 0;
      while (loopIndex < maxLoops && !stopFlagRef.current) {
        for (let index = 0; index < commands.length; index++) {
          if (stopFlagRef.current) break;
          if (mountedRef.current) {
            setCurrentIndex(index);
            setLoopProgress({ current: loopIndex, total: loopCount === 0 ? -1 : loopCount });
          }

          await onSend(commands[index]);
          if (stopFlagRef.current) break;

          const isLastCommandOfLastLoop = loopIndex === maxLoops - 1 && index === commands.length - 1;
          const delay = Math.max(0, commands[index].delay);
          if (delay > 0 && !isLastCommandOfLastLoop) {
            const wait = sleep(delay);
            cancelSleepRef.current = wait.cancel;
            try {
              await wait.promise;
            } catch {
              break;
            } finally {
              if (cancelSleepRef.current === wait.cancel) cancelSleepRef.current = null;
            }
          }
        }
        if (!stopFlagRef.current) loopIndex += 1;
      }
    } catch {
      // onSend owns user-facing error reporting; an error terminates this execution chain.
    } finally {
      runningRef.current = false;
      cancelSleepRef.current = null;
      if (mountedRef.current) {
        setIsRunning(false);
        setCurrentIndex(null);
        setLoopProgress(null);
      }
    }
  }, [onSend]);

  return { isRunning, currentIndex, loopProgress, start, stop };
}
