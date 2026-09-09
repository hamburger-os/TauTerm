import { useCallback, useEffect, useMemo, useState } from "react";

export interface ToolHistoryEntry {
  value: string;
  pinned: boolean;
  lastUsedAt: number;
}

const MAX_RECENT = 10;

export function useToolInputHistory(
  value: string,
  enabled: boolean,
  debounceMs = 900,
) {
  const [entries, setEntries] = useState<ToolHistoryEntry[]>([]);

  useEffect(() => {
    const normalized = value.trim();
    if (!enabled || !normalized) return;
    const timer = window.setTimeout(() => {
      setEntries((previous) => {
        const existing = previous.find((entry) => entry.value === normalized);
        const nextEntry: ToolHistoryEntry = {
          value: normalized,
          pinned: existing?.pinned ?? false,
          lastUsedAt: Date.now(),
        };
        const without = previous.filter((entry) => entry.value !== normalized);
        const pinned = without.filter((entry) => entry.pinned);
        const recent = without
          .filter((entry) => !entry.pinned)
          .sort((a, b) => b.lastUsedAt - a.lastUsedAt)
          .slice(0, Math.max(0, MAX_RECENT - (nextEntry.pinned ? 0 : 1)));

        return nextEntry.pinned
          ? [nextEntry, ...pinned, ...recent]
          : [...pinned, nextEntry, ...recent];
      });
    }, debounceMs);
    return () => window.clearTimeout(timer);
  }, [debounceMs, enabled, value]);

  const togglePinned = useCallback((entryValue: string) => {
    setEntries((previous) =>
      previous.map((entry) =>
        entry.value === entryValue
          ? { ...entry, pinned: !entry.pinned }
          : entry
      )
    );
  }, []);

  const clearRecent = useCallback(() => {
    setEntries((previous) => previous.filter((entry) => entry.pinned));
  }, []);

  const sortedEntries = useMemo(
    () => [...entries].sort((a, b) => {
      if (a.pinned !== b.pinned) return a.pinned ? -1 : 1;
      return b.lastUsedAt - a.lastUsedAt;
    }),
    [entries],
  );

  return {
    entries: sortedEntries,
    togglePinned,
    clearRecent,
  };
}
