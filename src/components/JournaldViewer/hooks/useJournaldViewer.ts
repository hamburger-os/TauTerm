import { useCallback, useState } from "react";
import type {
  DisplayMode,
  JournaldErrorPayload,
  JournaldFilter,
  SubTab,
} from "../types";
import { useJournalExport } from "./useJournalExport";
import { useJournalHistory } from "./useJournalHistory";
import { useJournalStream } from "./useJournalStream";

export interface UseJournaldViewerReturn {
  subTab: SubTab;
  entries: ReturnType<typeof useJournalStream>["entries"];
  loading: boolean;
  error: JournaldErrorPayload | null;
  filter: JournaldFilter;
  displayMode: DisplayMode;
  isStreaming: boolean;
  nextCursor: string | null;
  hasMore: boolean;
  totalLoaded: number;
  dropped: number;

  toggleStreaming: () => Promise<void>;
  runHistoryQuery: () => Promise<void>;
  stopStreaming: () => Promise<void>;
  queryHistory: (append?: boolean) => Promise<void>;
  setFilter: (partial: Partial<JournaldFilter>) => void;
  setDisplayMode: (mode: DisplayMode) => void;
  setSubTab: (tab: SubTab) => void;
  clearEntries: () => void;
  clearError: () => void;

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
  const [filter, setFilterState] = useState<JournaldFilter>({ searchMode: "literal" });
  const [displayMode, setDisplayMode] = useState<DisplayMode>("compact");

  const stream = useJournalStream(
    sessionId,
    isConnected,
    subTab === "realtime",
    filter,
  );
  const history = useJournalHistory(sessionId, isConnected);
  const exporter = useJournalExport(sessionId, isConnected);

  const setFilter = useCallback(
    (partial: Partial<JournaldFilter>) => {
      setFilterState((previous) => ({ ...previous, ...partial }));
      history.invalidate();
    },
    [history.invalidate],
  );

  const queryHistory = useCallback(
    async (append = false) => {
      await history.query(filter, append);
    },
    [filter, history.query],
  );

  const runHistoryQuery = useCallback(async () => {
    await history.query(filter, false);
  }, [filter, history.query]);

  const toggleStreaming = useCallback(async () => {
    await stream.toggle(filter);
  }, [filter, stream.toggle]);

  const startExport = useCallback(async () => {
    await exporter.start(filter);
  }, [exporter.start, filter]);

  const clearEntries = useCallback(() => {
    if (subTab === "realtime") {
      stream.clear();
    } else {
      history.clear();
    }
  }, [history.clear, stream.clear, subTab]);

  const clearError = useCallback(() => {
    stream.clearError();
    history.clearError();
    exporter.clearError();
  }, [exporter.clearError, history.clearError, stream.clearError]);

  const realtime = subTab === "realtime";
  return {
    subTab,
    entries: realtime ? stream.entries : history.entries,
    loading: realtime ? stream.loading : history.loading,
    error: realtime ? stream.error : history.error ?? exporter.error,
    filter,
    displayMode,
    isStreaming: stream.isStreaming,
    nextCursor: history.nextCursor,
    hasMore: history.hasMore,
    totalLoaded: realtime ? stream.totalLoaded : history.totalLoaded,
    dropped: realtime ? stream.dropped : 0,

    toggleStreaming,
    runHistoryQuery,
    stopStreaming: stream.stop,
    queryHistory,
    setFilter,
    setDisplayMode,
    setSubTab,
    clearEntries,
    clearError,

    exporting: exporter.exporting,
    exportLoaded: exporter.exportLoaded,
    startExport,
    cancelExport: exporter.cancel,
  };
}
