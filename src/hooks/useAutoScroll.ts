import { useCallback, useEffect, useRef, useState } from "react";

/**
 * 通用"自动滚底 + 浮动回到底部按钮"逻辑。
 *
 * 与 DualPane 行为一致：内容在底部时新数据自动滚底；用户向上滚动离开底部时
 * 暂停跟随并浮现"回到底部"按钮；点击按钮滚动到底并恢复跟随。
 * 供网络调试的 UDP 报文网格与 TCP Text/Hex 单栏复用。
 *
 * @param data 滚动内容的数据源；其引用变化（如追加行后的新数组）触发自动滚底
 */
export function useAutoScroll<T extends HTMLElement = HTMLDivElement>(data: readonly unknown[]) {
  const scrollRef = useRef<T>(null);
  const autoScrollRef = useRef(true);
  const [isAtBottom, setIsAtBottom] = useState(true);

  const handleScroll = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 30;
    autoScrollRef.current = atBottom;
    setIsAtBottom(atBottom);
  }, []);

  useEffect(() => {
    if (!autoScrollRef.current) return;
    const el = scrollRef.current;
    if (!el) return;
    el.scrollTop = el.scrollHeight;
    // 设置 scrollTop 会触发 scroll 事件，且可能在布局稳定前触发，导致误判非底部；
    // 延迟到下一帧再确认状态。
    requestAnimationFrame(() => {
      autoScrollRef.current = true;
      setIsAtBottom(true);
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [data]);

  const scrollToBottom = useCallback(() => {
    const el = scrollRef.current;
    if (el) el.scrollTop = el.scrollHeight;
    requestAnimationFrame(() => {
      autoScrollRef.current = true;
      setIsAtBottom(true);
    });
  }, []);

  return { scrollRef, isAtBottom, handleScroll, scrollToBottom };
}
