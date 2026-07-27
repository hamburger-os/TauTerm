import { useState, useCallback, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  JournalEntry,
  JournaldFilter,
  JournaldQueryResponse,
  DisplayMode,
  SubTab,
} from "../types";
import { MAX_ENTRIES } from "../types";

/** 按 __REALTIME_TIMESTAMP 降序排序（最新在顶部） */
function sortEntriesDesc(entries: JournalEntry[]): JournalEntry[] {
  return [...entries].sort((a, b) => {
    const ta = parseInt(a.__REALTIME_TIMESTAMP ?? "0", 10);
    const tb = parseInt(b.__REALTIME_TIMESTAMP ?? "0", 10);
    return tb - ta;
  });
}

export interface UseJournaldViewerReturn {
  subTab: SubTab;
  entries: JournalEntry[];
  loading: boolean;
  error: string | null;
  filter: JournaldFilter;
  displayMode: DisplayMode;
  isStreaming: boolean;
  nextCursor: string | null;
  hasMore: boolean;
  totalLoaded: number;

  toggleStreaming: () => Promise<void>;
  runHistoryQuery: () => Promise<void>;
  stopStreaming: () => Promise<void>;
  queryHistory: (append?: boolean) => Promise<void>;
  setFilter: (partial: Partial<JournaldFilter>) => void;
  setDisplayMode: (mode: DisplayMode) => void;
  setSubTab: (tab: SubTab) => void;
  clearEntries: () => void;
  clearError: () => void;
}

export function useJournaldViewer(
  sessionId: string,
  isConnected: boolean,
): UseJournaldViewerReturn {
  const [subTab, setSubTab] = useState<SubTab>("realtime");
  const [entries, setEntries] = useState<JournalEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilterState] = useState<JournaldFilter>({});
  const [displayMode, setDisplayMode] = useState<DisplayMode>("compact");
  const [isStreaming, setIsStreaming] = useState(false);
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  const [hasMore, setHasMore] = useState(false);
  const [totalLoaded, setTotalLoaded] = useState(0);

  const unlistenRef = useRef<UnlistenFn | null>(null);
  const isStreamingRef = useRef(false);
  const pendingRef = useRef<JournalEntry[]>([]);
  const flushTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const nextCursorRef = useRef(nextCursor);
  nextCursorRef.current = nextCursor;

  // ── 批量刷新待处理条目（100ms 窗口，减少 React 重渲染）──
  const FLUSH_INTERVAL = 100; // ms

  const flushPendingBatch = useCallback(() => {
    const batch = pendingRef.current.splice(0);
    if (batch.length === 0) return;
    setEntries((prev) => {
      const next = [...batch.reverse(), ...prev];
      return next.length > MAX_ENTRIES ? next.slice(0, MAX_ENTRIES) : next;
    });
    setTotalLoaded((prev) => prev + batch.length);
  }, []);

  // ── 事件监听管理 ──
  const setupEventListener = useCallback(async () => {
    if (unlistenRef.current) return;

    const collected: UnlistenFn[] = [];
    // 组合取消函数 — 任一 listen 失败时能清理已注册的监听器
    const combinedUnlisten = () => collected.forEach(fn => fn());
    try {
      collected.push(await listen<{
        session_id: string;
        entry: JournalEntry;
      }>("journald:entry", (event) => {
        if (event.payload.session_id !== sessionId) return;
        pendingRef.current.push(event.payload.entry);
        if (!flushTimerRef.current) {
          flushTimerRef.current = setTimeout(() => {
            flushTimerRef.current = null;
            flushPendingBatch();
          }, FLUSH_INTERVAL);
        }
      }));

      collected.push(await listen<{
        session_id: string;
        error: string;
      }>("journald:error", (event) => {
        if (event.payload.session_id !== sessionId) return;
        setError(event.payload.error);
        setIsStreaming(false);
        isStreamingRef.current = false;
      }));

      collected.push(await listen<{
        session_id: string;
        reason: string;
      }>("journald:stream-ended", (event) => {
        if (event.payload.session_id !== sessionId) return;
        setIsStreaming(false);
        isStreamingRef.current = false;
      }));

      unlistenRef.current = combinedUnlisten;
    } catch (e) {
      // 任一 listen 失败，清理已注册的监听器防止泄露
      combinedUnlisten();
      throw e;
    }
  }, [sessionId]);

  const teardownEventListener = useCallback(() => {
    unlistenRef.current?.();
    unlistenRef.current = null;
    if (flushTimerRef.current) {
      clearTimeout(flushTimerRef.current);
      flushTimerRef.current = null;
    }
    flushPendingBatch();
  }, [flushPendingBatch]);

  // ── 实时追踪 ──
  const startStreaming = useCallback(async (filters: JournaldFilter) => {
    if (!isConnected || isStreamingRef.current) return;
    setError(null);
    setLoading(true);
    try {
      await setupEventListener();
      await invoke<void>("start_journald_stream", {
        sessionId,
        level: filters.level ?? null,
        keyword: filters.keyword || null,
        unit: filters.unit || null,
        kernelOnly: filters.kernelOnly ?? false,
      });
      setIsStreaming(true);
      isStreamingRef.current = true;
    } catch (e) {
      setError(String(e));
    }
    setLoading(false);
  }, [isConnected, sessionId, setupEventListener]);

  // Filter ref for toggleStreaming to read latest without dep churn
  const filterRef = useRef(filter);
  filterRef.current = filter;

  const stopStreaming = useCallback(async () => {
    try {
      await invoke<void>("stop_journald_stream", { sessionId });
    } catch {
      // 忽略后台错误（流可能已经停止）
    }
    setIsStreaming(false);
    isStreamingRef.current = false;
    teardownEventListener();
  }, [sessionId, teardownEventListener]);

  // ── 历史查询 ──
  const queryHistory = useCallback(
    async (append = false) => {
      if (!isConnected) return;
      setError(null);
      setLoading(true);
      try {
        const response = await invoke<JournaldQueryResponse>(
          "journald_query_cmd",
          {
            sessionId,
            level: filter.level ?? null,
            keyword: filter.keyword || null,
            unit: filter.unit || null,
            kernelOnly: filter.kernelOnly ?? false,
            since: filter.since ?? null,
            until: filter.until ?? null,
            cursor: append ? nextCursorRef.current : null,
            limit: 100,
          },
        );
        if (append) {
          setEntries((prev) => sortEntriesDesc([...prev, ...response.entries]));
        } else {
          setEntries(sortEntriesDesc(response.entries));
        }
        setNextCursor(response.next_cursor);
        setHasMore(response.has_more);
        setTotalLoaded(
          (prev) => (append ? prev : 0) + response.entries.length,
        );
      } catch (e) {
        setError(String(e));
      }
      setLoading(false);
    },
    [isConnected, sessionId, filter],
  );

  const toggleStreaming = useCallback(async () => {
    if (isStreamingRef.current) {
      await stopStreaming();
    } else {
      await startStreaming(filterRef.current);
    }
  }, [stopStreaming, startStreaming]);

  const runHistoryQuery = useCallback(async () => {
    await queryHistory(false);
  }, [queryHistory]);

  // ── 过滤条件变更 → 重置查询 ──
  const setFilter = useCallback(
    (partial: Partial<JournaldFilter>) => {
      setFilterState((prev) => ({ ...prev, ...partial }));
      setNextCursor(null);
      setHasMore(false);
    },
    [],
  );

  const clearEntries = useCallback(() => {
    setEntries([]);
    setNextCursor(null);
    setHasMore(false);
    setTotalLoaded(0);
  }, []);

  const clearError = useCallback(() => setError(null), []);

  // ── 连接状态变化 ──
  useEffect(() => {
    if (!isConnected) {
      clearEntries();
      if (isStreamingRef.current) {
        stopStreaming();
      }
      setError(null);
    }
  }, [isConnected, clearEntries, stopStreaming]);

  // ── subTab 切换：离开实时模式时停止流 ──
  useEffect(() => {
    if (subTab !== "realtime" && isStreamingRef.current) {
      stopStreaming();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [subTab]);

  // ── 实时模式过滤条件变更 → 300ms 防抖后重启流 ──
  const debounceTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const streamFilterKeyRef = useRef("");
  useEffect(() => {
    // 非流式场景下：清理防抖定时器，重置 key 追踪
    if (!isConnected || subTab !== "realtime" || !isStreaming) {
      streamFilterKeyRef.current = "";
      if (debounceTimerRef.current) {
        clearTimeout(debounceTimerRef.current);
        debounceTimerRef.current = null;
      }
      return;
    }
    const key = JSON.stringify({
      l: filter.level,
      k: filter.keyword,
      u: filter.unit,
      ko: filter.kernelOnly,
    });
    // 首次流启动时记录 key，跳过重启
    if (!streamFilterKeyRef.current) {
      streamFilterKeyRef.current = key;
      return;
    }
    // 过滤条件未变化 → 跳过
    if (streamFilterKeyRef.current === key) {
      return;
    }
    streamFilterKeyRef.current = key;

    // 300ms 防抖：连续变更时仅最后一次生效
    if (debounceTimerRef.current) {
      clearTimeout(debounceTimerRef.current);
    }
    debounceTimerRef.current = setTimeout(() => {
      debounceTimerRef.current = null;
      stopStreaming().then(() => startStreaming(filterRef.current));
    }, 300);

    return () => {
      if (debounceTimerRef.current) {
        clearTimeout(debounceTimerRef.current);
        debounceTimerRef.current = null;
      }
    };
  }, [
    filter.level, filter.keyword, filter.unit, filter.kernelOnly,
    subTab, isConnected, isStreaming,
    stopStreaming, startStreaming,
  ]);

  // ── 组件卸载清理 ──
  useEffect(() => {
    return () => {
      if (flushTimerRef.current) {
        clearTimeout(flushTimerRef.current);
        flushTimerRef.current = null;
      }
      if (debounceTimerRef.current) {
        clearTimeout(debounceTimerRef.current);
        debounceTimerRef.current = null;
      }
      if (isStreamingRef.current) {
        // 异步调用，不 await
        invoke<void>("stop_journald_stream", { sessionId }).catch(() => {});
      }
      teardownEventListener();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return {
    subTab,
    entries,
    loading,
    error,
    filter,
    displayMode,
    isStreaming,
    nextCursor,
    hasMore,
    totalLoaded,

    toggleStreaming,
    runHistoryQuery,
    stopStreaming,
    queryHistory,
    setFilter,
    setDisplayMode,
    setSubTab,
    clearEntries,
    clearError,
  };
}
