import { useCallback, useEffect, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { JournalEntry, JournaldErrorPayload, JournaldFilter } from "../types";
import { MAX_ENTRIES } from "../types";
import {
  normalizeJournaldError,
  startJournalStream,
  stopJournalStream,
} from "./journaldClient";

interface JournalBatchEvent {
  session_id: string;
  entries: JournalEntry[];
  dropped: number;
}

interface JournalErrorEvent {
  session_id: string;
  error: JournaldErrorPayload;
}

interface JournalEndedEvent {
  session_id: string;
  reason: string;
}

export interface UseJournalStreamReturn {
  entries: JournalEntry[];
  loading: boolean;
  error: JournaldErrorPayload | null;
  isStreaming: boolean;
  totalLoaded: number;
  dropped: number;
  start: (filter: JournaldFilter) => Promise<void>;
  stop: () => Promise<void>;
  toggle: (filter: JournaldFilter) => Promise<void>;
  clear: () => void;
  clearError: () => void;
}

function filterKey(filter: JournaldFilter): string {
  return JSON.stringify({
    level: filter.level ?? null,
    keyword: filter.keyword?.trim() ?? "",
    searchMode: filter.searchMode ?? "literal",
    unit: filter.unit?.trim() ?? "",
    kernelOnly: filter.kernelOnly ?? false,
  });
}

export function useJournalStream(
  sessionId: string,
  isConnected: boolean,
  active: boolean,
  filter: JournaldFilter,
): UseJournalStreamReturn {
  const [entries, setEntries] = useState<JournalEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<JournaldErrorPayload | null>(null);
  const [isStreaming, setIsStreaming] = useState(false);
  const [totalLoaded, setTotalLoaded] = useState(0);
  const [dropped, setDropped] = useState(0);

  const streamingRef = useRef(false);
  const busyRef = useRef(false);
  const disposedRef = useRef(false);
  const activeRef = useRef(active);
  const connectedRef = useRef(isConnected);
  const generationRef = useRef(0);
  const listenerEpochRef = useRef(0);
  const runningFilterKeyRef = useRef("");
  const restartTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const unlistenersRef = useRef<UnlistenFn[]>([]);
  const listenersReadyRef = useRef<Promise<void> | null>(null);

  activeRef.current = active;
  connectedRef.current = isConnected;

  const clear = useCallback(() => {
    setEntries([]);
    setTotalLoaded(0);
    setDropped(0);
  }, []);

  const clearError = useCallback(() => setError(null), []);

  const ensureListeners = useCallback(async () => {
    if (listenersReadyRef.current) {
      return listenersReadyRef.current;
    }

    const listenerEpoch = listenerEpochRef.current;
    const setupPromise = (async () => {
      const collected: UnlistenFn[] = [];
      const isStale = () =>
        disposedRef.current || listenerEpoch !== listenerEpochRef.current;
      try {
        collected.push(
          await listen<JournalBatchEvent>("journald:batch", (event) => {
            if (isStale() || event.payload.session_id !== sessionId) return;
            const batch = event.payload.entries;
            if (batch.length > 0) {
              setEntries((previous) => {
                const next = [...batch].reverse().concat(previous);
                return next.length > MAX_ENTRIES ? next.slice(0, MAX_ENTRIES) : next;
              });
              setTotalLoaded((previous) => previous + batch.length);
            }
            if (event.payload.dropped > 0) {
              setDropped((previous) => previous + event.payload.dropped);
            }
          }),
        );
        collected.push(
          await listen<JournalErrorEvent>("journald:error", (event) => {
            if (isStale() || event.payload.session_id !== sessionId) return;
            setError(event.payload.error);
            setIsStreaming(false);
            streamingRef.current = false;
            runningFilterKeyRef.current = "";
          }),
        );
        collected.push(
          await listen<JournalEndedEvent>("journald:stream-ended", (event) => {
            if (isStale() || event.payload.session_id !== sessionId) return;
            setIsStreaming(false);
            streamingRef.current = false;
            runningFilterKeyRef.current = "";
          }),
        );
        if (isStale()) {
          collected.forEach((unlisten) => unlisten());
          return;
        }
        unlistenersRef.current = collected;
      } catch (listenerError) {
        collected.forEach((unlisten) => unlisten());
        throw listenerError;
      }
    })();

    listenersReadyRef.current = setupPromise;
    try {
      await setupPromise;
    } catch (listenerError) {
      if (listenersReadyRef.current === setupPromise) {
        listenersReadyRef.current = null;
      }
      throw listenerError;
    }
  }, [sessionId]);

  const start = useCallback(
    async (nextFilter: JournaldFilter) => {
      if (
        !isConnected ||
        !activeRef.current ||
        disposedRef.current ||
        busyRef.current ||
        streamingRef.current
      ) {
        return;
      }
      busyRef.current = true;
      const generation = ++generationRef.current;
      setLoading(true);
      setError(null);
      try {
        await ensureListeners();
        if (
          generation !== generationRef.current ||
          disposedRef.current ||
          !activeRef.current ||
          !connectedRef.current
        ) {
          return;
        }

        await startJournalStream(sessionId, nextFilter);
        if (
          generation !== generationRef.current ||
          disposedRef.current ||
          !activeRef.current ||
          !connectedRef.current
        ) {
          await stopJournalStream(sessionId).catch(() => undefined);
          return;
        }

        streamingRef.current = true;
        setIsStreaming(true);
        runningFilterKeyRef.current = filterKey(nextFilter);
      } catch (startError) {
        if (generation !== generationRef.current || disposedRef.current) return;
        streamingRef.current = false;
        setIsStreaming(false);
        runningFilterKeyRef.current = "";
        setError(normalizeJournaldError(startError));
      } finally {
        if (generation === generationRef.current && !disposedRef.current) {
          setLoading(false);
        }
        busyRef.current = false;
      }
    },
    [ensureListeners, isConnected, sessionId],
  );

  const stop = useCallback(async () => {
    if (busyRef.current) return;
    if (!streamingRef.current) {
      setIsStreaming(false);
      return;
    }
    busyRef.current = true;
    ++generationRef.current;
    setLoading(true);
    try {
      await stopJournalStream(sessionId);
    } catch (stopError) {
      if (!disposedRef.current) {
        setError(normalizeJournaldError(stopError));
      }
    } finally {
      streamingRef.current = false;
      runningFilterKeyRef.current = "";
      if (!disposedRef.current) {
        setIsStreaming(false);
        setLoading(false);
      }
      busyRef.current = false;
    }
  }, [sessionId]);

  const toggle = useCallback(
    async (nextFilter: JournaldFilter) => {
      if (streamingRef.current) {
        await stop();
      } else {
        await start(nextFilter);
      }
    },
    [start, stop],
  );

  useEffect(() => {
    if (!active || !isConnected) {
      if (restartTimerRef.current) {
        clearTimeout(restartTimerRef.current);
        restartTimerRef.current = null;
      }
      if (streamingRef.current && !busyRef.current) {
        void stop();
      }
      if (!isConnected) {
        clear();
        setError(null);
      }
      return;
    }

    if (!streamingRef.current) return;
    const nextKey = filterKey(filter);
    if (!runningFilterKeyRef.current || nextKey === runningFilterKeyRef.current) return;

    if (restartTimerRef.current) clearTimeout(restartTimerRef.current);
    restartTimerRef.current = setTimeout(() => {
      restartTimerRef.current = null;
      void (async () => {
        await stop();
        await start(filter);
      })();
    }, 300);

    return () => {
      if (restartTimerRef.current) {
        clearTimeout(restartTimerRef.current);
        restartTimerRef.current = null;
      }
    };
  }, [active, clear, filter, isConnected, isStreaming, start, stop]);

  useEffect(() => {
    disposedRef.current = false;
    ++listenerEpochRef.current;
    void ensureListeners().catch((listenerError) => {
      if (!disposedRef.current) {
        setError(normalizeJournaldError(listenerError));
      }
    });

    return () => {
      disposedRef.current = true;
      ++generationRef.current;
      ++listenerEpochRef.current;
      if (restartTimerRef.current) {
        clearTimeout(restartTimerRef.current);
        restartTimerRef.current = null;
      }
      unlistenersRef.current.forEach((unlisten) => unlisten());
      unlistenersRef.current = [];
      listenersReadyRef.current = null;
      if (streamingRef.current || busyRef.current) {
        void stopJournalStream(sessionId).catch(() => undefined);
      }
      streamingRef.current = false;
      runningFilterKeyRef.current = "";
    };
  }, [ensureListeners, sessionId]);

  return {
    entries,
    loading,
    error,
    isStreaming,
    totalLoaded,
    dropped,
    start,
    stop,
    toggle,
    clear,
    clearError,
  };
}
