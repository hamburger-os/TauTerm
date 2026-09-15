import { useEffect, useRef, useState } from "react";

export interface SessionTrafficRate {
  tx: number;
  rx: number;
}

/**
 * 从 Session 累计字节统计中派生短窗口速率。
 *
 * 速率采样属于共享 presentation metrics，不由 StatusBar 自己维护。
 * 其它轻量诊断/统计 UI 可以复用同一语义，避免各组件重复实现采样窗口。
 */
export function useSessionTrafficRate(
  sessionId: string | null | undefined,
  active: boolean,
  txBytes: number,
  rxBytes: number,
): SessionTrafficRate {
  const [rate, setRate] = useState<SessionTrafficRate>({ tx: 0, rx: 0 });
  const totalsRef = useRef({ tx: txBytes, rx: rxBytes });
  const lastSampleRef = useRef<{ tx: number; rx: number; ts: number } | null>(null);
  const windowRef = useRef<SessionTrafficRate[]>([]);

  totalsRef.current = { tx: txBytes, rx: rxBytes };

  useEffect(() => {
    lastSampleRef.current = null;
    windowRef.current = [];
    setRate({ tx: 0, rx: 0 });

    if (!sessionId || !active) return;

    const tick = () => {
      const now = Date.now();
      const current = totalsRef.current;
      const last = lastSampleRef.current;

      if (last) {
        const elapsedSeconds = (now - last.ts) / 1000;
        if (elapsedSeconds >= 0.5) {
          const sample = {
            tx: Math.max(0, (current.tx - last.tx) / elapsedSeconds),
            rx: Math.max(0, (current.rx - last.rx) / elapsedSeconds),
          };
          const samples = windowRef.current;
          samples.push(sample);
          if (samples.length > 3) samples.shift();
          setRate({
            tx: samples.reduce((sum, item) => sum + item.tx, 0) / samples.length,
            rx: samples.reduce((sum, item) => sum + item.rx, 0) / samples.length,
          });
        }
      }

      lastSampleRef.current = { ...current, ts: now };
    };

    tick();
    const timer = window.setInterval(tick, 1000);
    return () => window.clearInterval(timer);
  }, [sessionId, active]);

  return rate;
}
