import { useCallback, useEffect, useRef, useState } from "react";
import type { JournalEntry, JournaldErrorPayload, JournaldFilter } from "../types";
import { normalizeJournaldError, queryJournalHistory } from "./journaldClient";

export interface UseJournalHistoryReturn {
  entries: JournalEntry[];
  loading: boolean;
  error: JournaldErrorPayload | null;
  nextCursor: string | null;
  hasMore: boolean;
  totalLoaded: number;
  query: (filter: JournaldFilter, append?: boolean) => Promise<void>;
  invalidate: () => void;
  clear: () => void;
  clearError: () => void;
}

export function useJournalHistory(
  sessionId: string,
  isConnected: boolean,
): UseJournalHistoryReturn {
  const [entries, setEntries] = useState<JournalEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<JournaldErrorPayload | null>(null);
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  const [hasMore, setHasMore] = useState(false);
  const [totalLoaded, setTotalLoaded] = useState(0);

  const generationRef = useRef(0);
  const nextCursorRef = useRef<string | null>(null);

  const clear = useCallback(() => {
    ++generationRef.current;
    nextCursorRef.current = null;
    setEntries([]);
    setNextCursor(null);
    setHasMore(false);
    setTotalLoaded(0);
    setLoading(false);
  }, []);

  const invalidate = useCallback(() => {
    ++generationRef.current;
    nextCursorRef.current = null;
    setNextCursor(null);
    setHasMore(false);
    setLoading(false);
  }, []);

  const clearError = useCallback(() => setError(null), []);

  const query = useCallback(
    async (filter: JournaldFilter, append = false) => {
      if (!isConnected) return;
      const cursor = append ? nextCursorRef.current : null;
      if (append && !cursor) return;

      setLoading(true);
      setError(null);
      const generation = ++generationRef.current;
      try {
        const response = await queryJournalHistory(sessionId, filter, cursor);
        if (generation !== generationRef.current) return;

        if (append) {
          setEntries((previous) => previous.concat(response.entries));
          setTotalLoaded((previous) => previous + response.entries.length);
        } else {
          setEntries(response.entries);
          setTotalLoaded(response.entries.length);
        }
        nextCursorRef.current = response.next_cursor;
        setNextCursor(response.next_cursor);
        setHasMore(response.has_more);
      } catch (queryError) {
        if (generation === generationRef.current) {
          setError(normalizeJournaldError(queryError));
        }
      } finally {
        if (generation === generationRef.current) {
          setLoading(false);
        }
      }
    },
    [isConnected, sessionId],
  );

  useEffect(() => {
    if (!isConnected) {
      clear();
      setError(null);
    }
  }, [clear, isConnected]);

  return {
    entries,
    loading,
    error,
    nextCursor,
    hasMore,
    totalLoaded,
    query,
    invalidate,
    clear,
    clearError,
  };
}
