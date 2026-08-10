import { useState, useCallback, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { save } from "@tauri-apps/plugin-dialog";
import type {
  JournalEntry,
  JournaldFilter,
  JournaldQueryResponse,
  DisplayMode,
  SubTab,
} from "../types";
import { MAX_ENTRIES } from "../types";
import { formatDateCompact } from "../../../utils/format";

/** 按 __REALTIME_TIMESTAMP 降序排序（最新在顶部） */
function sortEntriesDesc(entries: JournalEntry[]): JournalEntry[] {
  return [...entries].sort((a, b) => {
    const ta = parseInt(a.__REALTIME_TIMESTAMP ?? "0", 10);
    const tb = parseInt(b.__REALTIME_TIMESTAMP ?? "0", 10);
    return tb - ta;
  });
}

/** 生成默认导出文件名 */
function defaultExportName(): string {
  return `journald_export_${formatDateCompact(new Date())}.json`;
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

  // 导出
  exporting: boolean;
  exportLoaded: number;
  startExport: () => Promise<void>;
  cancelExport: () => Promise<void>;
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

  // 导出状态
  const [exporting, setExporting] = useState(false);
  const [exportLoaded, setExportLoaded] = useState(0);
  // 与 state 同步的 ref，供 unmount 清理读取最新值
  const exportingRef = useRef(false);
  const setExportingState = useCallback((v: boolean) => {
    exportingRef.current = v;
    setExporting(v);
  }, []);

  const unlistenRef = useRef<UnlistenFn | null>(null);
  const isStreamingRef = useRef(false);
  const pendingRef = useRef<JournalEntry[]>([]);
  const flushTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const nextCursorRef = useRef(nextCursor);
  nextCursorRef.current = nextCursor;

  // 导出事件监听
  const exportUnlistenRef = useRef<UnlistenFn | null>(null);

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
    // 组合取消函数 — 任一 listen 失败时能清理已注册的监听器；
    // 事件回调中调用时同时将 ref 置 null，防止重复拆除
    const combinedUnlisten = () => {
      collected.forEach(fn => fn());
      unlistenRef.current = null;
    };
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

  // ── 导出事件监听 ──
  const setupExportListeners = useCallback(async () => {
    if (exportUnlistenRef.current) return;

    const collected: UnlistenFn[] = [];
    // 组合取消函数 — 任一 listen 失败时能清理已注册的监听器；
    // 事件回调中调用时同时将 ref 置 null，防止二次导出时 setup 幂等检查误跳过
    const combinedUnlisten = () => {
      collected.forEach(fn => fn());
      exportUnlistenRef.current = null;
    };
    try {
      collected.push(await listen<{
        session_id: string;
        loaded: number;
      }>("journald:export-progress", (event) => {
        if (event.payload.session_id !== sessionId) return;
        setExportLoaded(event.payload.loaded);
      }));

      collected.push(await listen<{
        session_id: string;
        file_path: string;
        total: number;
      }>("journald:export-complete", (event) => {
        if (event.payload.session_id !== sessionId) return;
        setExportLoaded(event.payload.total);
        setExportingState(false);
        combinedUnlisten();
      }));

      collected.push(await listen<{
        session_id: string;
        error: string;
      }>("journald:export-error", (event) => {
        if (event.payload.session_id !== sessionId) return;
        setError(event.payload.error);
        setExportingState(false);
        combinedUnlisten();
      }));

      collected.push(await listen<{
        session_id: string;
      }>("journald:export-cancelled", (event) => {
        if (event.payload.session_id !== sessionId) return;
        // 后端任务已真正终止（注册表已释放），此时才允许重新导出
        setExportingState(false);
        combinedUnlisten();
      }));

      exportUnlistenRef.current = combinedUnlisten;
    } catch (e) {
      combinedUnlisten();
      throw e;
    }
  }, [sessionId, setExportingState]);

  const teardownExportListeners = useCallback(() => {
    exportUnlistenRef.current?.();
    exportUnlistenRef.current = null;
  }, []);

  // ── 实时追踪 ──
  const startStreaming = useCallback(async (filters: JournaldFilter) => {
    if (!isConnected || isStreamingRef.current) return;
    setError(null);
    setLoading(true);
    // 前置置位，防止 invoke 挂起期间重复触发（快速双击导致后端 register 拒绝）
    isStreamingRef.current = true;
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
    } catch (e) {
      const msg = String(e);
      if (msg.includes("已在运行中")) {
        // 后端仍有活跃流（如卸载重挂载后旧任务的停止尚在途）：
        // 接管其生命周期，不显示错误横幅，等 journald:stream-ended 事件统一复位
        setIsStreaming(true);
        isStreamingRef.current = true;
      } else {
        isStreamingRef.current = false;
        setIsStreaming(false);
        setError(msg);
      }
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

  // ── 导出 ──
  const startExport = useCallback(async () => {
    if (exporting || !isConnected) return;

    // 弹出保存对话框
    const filePath = await save({
      defaultPath: defaultExportName(),
      filters: [{ name: "JSON", extensions: ["json"] }],
    });
    if (!filePath) return; // 用户取消

    setExportingState(true);
    setExportLoaded(0);
    setError(null);

    try {
      await setupExportListeners();
      await invoke<void>("start_journald_export", {
        sessionId,
        filePath,
        level: filter.level ?? null,
        keyword: filter.keyword || null,
        unit: filter.unit || null,
        kernelOnly: filter.kernelOnly ?? false,
        since: filter.since ?? null,
        until: filter.until ?? null,
      });
    } catch (e) {
      setError(String(e));
      setExportingState(false);
      // 监听器已注册但后端未接受导出（如"已在运行中"）→ 拆除避免残留
      teardownExportListeners();
    }
  }, [exporting, isConnected, sessionId, filter, setupExportListeners, setExportingState, teardownExportListeners]);

  const cancelExport = useCallback(async () => {
    try {
      await invoke<void>("stop_journald_export", { sessionId });
    } catch {
      // 忽略后台错误（导出任务可能已结束）
    }
    // 注意：不在此处复位 exporting / 拆除监听器。
    // 后端任务真正退出（ExportGuard 释放注册表）后 emit journald:export-cancelled，
    // 由监听器统一复位 — 避免用户立即重导时后端仍报"导出已在运行中"。
  }, [sessionId]);

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
      teardownExportListeners();
      // 取消正在进行中的导出（仅当确有导出时，避免无谓 IPC）
      if (exportingRef.current) {
        invoke<void>("stop_journald_export", { sessionId }).catch(() => {});
      }
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

    exporting,
    exportLoaded,
    startExport,
    cancelExport,
  };
}
