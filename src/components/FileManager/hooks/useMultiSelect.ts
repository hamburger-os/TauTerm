import { useState, useCallback, useMemo } from 'react';
import { SftpEntry } from '../types';

export interface UseMultiSelectReturn {
  selectedPaths: Set<string>;
  lastClickedIndex: number | null;
  selectedEntries: SftpEntry[];
  handleClick: (entry: SftpEntry, index: number, ctrlKey: boolean, shiftKey: boolean) => void;
  handleRightClick: (entry: SftpEntry, ctrlKey: boolean) => void;
  selectAll: (entries: SftpEntry[]) => void;
  clearSelection: () => void;
  isSelected: (path: string) => boolean;
  selectionCount: number;
}

export function useMultiSelect(entries: SftpEntry[]): UseMultiSelectReturn {
  const [selectedPaths, setSelectedPaths] = useState<Set<string>>(new Set());
  const [lastClickedIndex, setLastClickedIndex] = useState<number | null>(null);

  const handleClick = useCallback(
    (entry: SftpEntry, index: number, ctrlKey: boolean, shiftKey: boolean) => {
      setSelectedPaths(prev => {
        const next = new Set(prev);

        if (ctrlKey) {
          // Toggle the clicked entry
          if (next.has(entry.path)) {
            next.delete(entry.path);
          } else {
            next.add(entry.path);
          }
          setLastClickedIndex(index);
          return next;
        }

        if (shiftKey && lastClickedIndex !== null) {
          // Range select from lastClickedIndex to current index
          const start = Math.min(lastClickedIndex, index);
          const end = Math.max(lastClickedIndex, index);
          for (let i = start; i <= end; i++) {
            if (entries[i]) {
              next.add(entries[i].path);
            }
          }
          // Keep lastClickedIndex unchanged for extending the range
          return next;
        }

        // Single select: clear and select only this entry
        next.clear();
        next.add(entry.path);
        setLastClickedIndex(index);
        return next;
      });
    },
    [entries, lastClickedIndex]
  );

  const selectAll = useCallback((allEntries: SftpEntry[]) => {
    setSelectedPaths(new Set(allEntries.map(e => e.path)));
    setLastClickedIndex(null);
  }, []);

  const clearSelection = useCallback(() => {
    setSelectedPaths(new Set());
    setLastClickedIndex(null);
  }, []);

  // ── Right-click: auto-select only if not already in selection ──
  //
  // Matches Windows Explorer / macOS Finder behavior:
  // - Right-click unselected file → clear + select it (single-item menu)
  // - Right-click file that's already in multi-select → keep selection (batch menu)
  // - Ctrl+right-click → toggle file in/out of selection
  const handleRightClick = useCallback(
    (entry: SftpEntry, ctrlKey: boolean) => {
      setSelectedPaths(prev => {
        const next = new Set(prev);

        if (ctrlKey) {
          if (next.has(entry.path)) {
            next.delete(entry.path);
          } else {
            next.add(entry.path);
          }
          setLastClickedIndex(null);
          return next;
        }

        if (!next.has(entry.path)) {
          next.clear();
          next.add(entry.path);
        }
        setLastClickedIndex(null);
        return next;
      });
    },
    []
  );

  const isSelected = useCallback(
    (path: string) => selectedPaths.has(path),
    [selectedPaths]
  );

  const selectedEntries = useMemo(
    () => entries.filter(e => selectedPaths.has(e.path)),
    [entries, selectedPaths]
  );

  return useMemo(() => ({
    selectedPaths,
    lastClickedIndex,
    selectedEntries,
    handleClick,
    handleRightClick,
    selectAll,
    clearSelection,
    isSelected,
    selectionCount: selectedPaths.size,
  }), [selectedPaths, lastClickedIndex, selectedEntries, handleClick, isSelected]);
}
