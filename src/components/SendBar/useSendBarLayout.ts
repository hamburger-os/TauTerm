import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
  type MouseEvent as ReactMouseEvent,
  type RefObject,
} from "react";
import {
  clampSendBarBodyHeight,
  getSendBarHostHeightCss,
  getSendBarHostMaxHeightCss,
  getSendBarHostMinHeightCss,
} from "./sendBarLayout";

interface SendBarLayoutMetrics {
  bodyMinHeight: number;
  targetBarHeight: number;
}

interface UseSendBarLayoutOptions {
  containerRef: RefObject<HTMLDivElement>;
  showTargetBar: boolean;
}

interface UseSendBarLayoutResult {
  isResizing: boolean;
  hostStyle: CSSProperties;
  onResizeStart: (event: ReactMouseEvent) => void;
}

/** Resolve a CSS length custom property (including calc()) into CSS pixels. */
function resolveCssLengthPx(varName: string): number | null {
  const probe = document.createElement("div");
  probe.style.cssText = [
    "position:absolute",
    "visibility:hidden",
    "pointer-events:none",
    `height:var(${varName})`,
  ].join(";");
  document.body.appendChild(probe);
  const height = probe.getBoundingClientRect().height;
  probe.remove();
  return Number.isFinite(height) && height > 0 ? height : null;
}

function readLayoutMetrics(): SendBarLayoutMetrics | null {
  const bodyMinHeight = resolveCssLengthPx("--sendbar-min-height");
  const targetBarHeight = resolveCssLengthPx("--sendbar-targetbar-height");
  if (bodyMinHeight === null || targetBarHeight === null) return null;
  return { bodyMinHeight, targetBarHeight };
}

/**
 * Owns the global SendBar splitter geometry.
 *
 * The user-adjustable value is the SendBar body height in CSS pixels. The
 * canonical minimum stays identical to the CSS button-stack geometry, while a
 * visible TargetBar remains a fixed additive row. This deliberately avoids
 * percentage quantization, so dragging back to the minimum returns to the exact
 * startup geometry.
 */
export function useSendBarLayout({
  containerRef,
  showTargetBar,
}: UseSendBarLayoutOptions): UseSendBarLayoutResult {
  const [metrics, setMetrics] = useState<SendBarLayoutMetrics | null>(null);
  const [bodyHeight, setBodyHeight] = useState<number | null>(null);
  const [isResizing, setIsResizing] = useState(false);
  const startYRef = useRef(0);
  const startBodyHeightRef = useRef(0);

  useLayoutEffect(() => {
    const nextMetrics = readLayoutMetrics();
    if (nextMetrics === null) return;
    setMetrics(nextMetrics);
    setBodyHeight(nextMetrics.bodyMinHeight);
  }, []);

  const clampToContainer = useCallback((desiredBodyHeight: number): number => {
    if (metrics === null) return desiredBodyHeight;
    const containerHeight = containerRef.current?.clientHeight ?? 0;
    return clampSendBarBodyHeight(
      desiredBodyHeight,
      containerHeight,
      metrics.bodyMinHeight,
      showTargetBar ? metrics.targetBarHeight : 0,
    );
  }, [containerRef, metrics, showTargetBar]);

  useEffect(() => {
    if (metrics === null) return;
    const container = containerRef.current;
    if (!container) return;

    const normalizeHeight = () => {
      setBodyHeight(current => clampSendBarBodyHeight(
        current ?? metrics.bodyMinHeight,
        container.clientHeight,
        metrics.bodyMinHeight,
        showTargetBar ? metrics.targetBarHeight : 0,
      ));
    };

    normalizeHeight();
    const observer = new ResizeObserver(normalizeHeight);
    observer.observe(container);
    return () => observer.disconnect();
  }, [containerRef, metrics, showTargetBar]);

  const onResizeStart = useCallback((event: ReactMouseEvent) => {
    event.preventDefault();
    if (metrics === null) return;

    const initialBodyHeight = clampToContainer(bodyHeight ?? metrics.bodyMinHeight);
    startYRef.current = event.clientY;
    startBodyHeightRef.current = initialBodyHeight;
    setBodyHeight(initialBodyHeight);
    setIsResizing(true);
  }, [bodyHeight, clampToContainer, metrics]);

  useEffect(() => {
    if (!isResizing) return;

    const handleMove = (event: MouseEvent) => {
      const desiredBodyHeight = startBodyHeightRef.current + (startYRef.current - event.clientY);
      setBodyHeight(clampToContainer(desiredBodyHeight));
    };
    const handleUp = () => setIsResizing(false);
    const previousCursor = document.body.style.cursor;
    const previousUserSelect = document.body.style.userSelect;

    document.addEventListener("mousemove", handleMove);
    document.addEventListener("mouseup", handleUp);
    document.body.style.cursor = "row-resize";
    document.body.style.userSelect = "none";

    return () => {
      document.removeEventListener("mousemove", handleMove);
      document.removeEventListener("mouseup", handleUp);
      document.body.style.cursor = previousCursor;
      document.body.style.userSelect = previousUserSelect;
    };
  }, [clampToContainer, isResizing]);

  const hostHeight = getSendBarHostHeightCss(bodyHeight, showTargetBar);
  const hostStyle: CSSProperties = {
    flex: `0 0 ${hostHeight}`,
    minHeight: getSendBarHostMinHeightCss(showTargetBar),
    maxHeight: getSendBarHostMaxHeightCss(),
    display: "flex",
    flexDirection: "column",
  };

  return { isResizing, hostStyle, onResizeStart };
}
