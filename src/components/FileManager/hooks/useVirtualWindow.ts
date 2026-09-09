import { useCallback, useEffect, useMemo, useRef, useState } from "react";

interface VirtualWindowOptions {
  count: number;
  itemSize: number;
  leadingSize?: number;
  overscan?: number;
  threshold?: number;
}

export function useVirtualWindow({
  count,
  itemSize,
  leadingSize = 0,
  overscan = 6,
  threshold = 300,
}: VirtualWindowOptions) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [viewportHeight, setViewportHeight] = useState(0);
  const [viewportWidth, setViewportWidth] = useState(0);
  const [scrollTop, setScrollTop] = useState(0);
  const enabled = count >= threshold;

  useEffect(() => {
    const node = containerRef.current;
    if (!node) return;

    const update = () => {
      setViewportHeight(node.clientHeight);
      setViewportWidth(node.clientWidth);
    };
    update();

    const observer = new ResizeObserver(update);
    observer.observe(node);
    return () => observer.disconnect();
  }, []);

  const onScroll = useCallback((event: React.UIEvent<HTMLDivElement>) => {
    if (!enabled) return;
    setScrollTop(event.currentTarget.scrollTop);
  }, [enabled]);

  const range = useMemo(() => {
    if (!enabled) {
      return { start: 0, end: count, offset: 0, totalSize: count * itemSize };
    }
    const relativeTop = Math.max(0, scrollTop - leadingSize);
    const start = Math.max(0, Math.floor(relativeTop / itemSize) - overscan);
    const visibleCount = Math.ceil(Math.max(viewportHeight, itemSize) / itemSize);
    const end = Math.min(count, start + visibleCount + overscan * 2);
    return {
      start,
      end,
      offset: start * itemSize,
      totalSize: count * itemSize,
    };
  }, [count, enabled, itemSize, leadingSize, overscan, scrollTop, viewportHeight]);

  const scrollIndexIntoView = useCallback((index: number) => {
    const node = containerRef.current;
    if (!node || index < 0) return;

    const itemTop = leadingSize + index * itemSize;
    const itemBottom = itemTop + itemSize;
    if (itemTop < node.scrollTop) {
      node.scrollTop = itemTop;
    } else if (itemBottom > node.scrollTop + node.clientHeight) {
      node.scrollTop = itemBottom - node.clientHeight;
    }
    if (enabled) setScrollTop(node.scrollTop);
  }, [enabled, itemSize, leadingSize]);

  return {
    containerRef,
    enabled,
    viewportHeight,
    viewportWidth,
    onScroll,
    scrollIndexIntoView,
    ...range,
  };
}
