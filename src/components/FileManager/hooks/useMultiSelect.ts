import { useState, useCallback, useMemo } from 'react';
import { SftpEntry } from '../types';

export interface UseMultiSelectReturn {
  selectedPaths: Set<string>;
  lastClickedIndex: number | null;
  selectedEntries: SftpEntry[];
  /** 上级目录（`..`）是否处于选中态；与文件路径选择互斥 */
  parentSelected: boolean;
  handleClick: (entry: SftpEntry, index: number, additiveKey: boolean, shiftKey: boolean) => void;
  handleRightClick: (entry: SftpEntry) => void;
  selectAll: (entries: SftpEntry[]) => void;
  /** 选中上级目录（清除文件选择） */
  selectParent: () => void;
  clearSelection: () => void;
  isSelected: (path: string) => boolean;
  selectionCount: number;
}

export function useMultiSelect(entries: SftpEntry[]): UseMultiSelectReturn {
  const [selectedPaths, setSelectedPaths] = useState<Set<string>>(new Set());
  // Keep the range-selection anchor by stable entry identity instead of by row
  // number. Sorting or refreshing the directory may reorder entries between
  // clicks; an index anchor would then select a range from the wrong file.
  const [lastClickedPath, setLastClickedPath] = useState<string | null>(null);
  const [parentSelected, setParentSelected] = useState(false);

  const lastClickedIndex = useMemo(() => {
    if (lastClickedPath === null) return null;
    const index = entries.findIndex((entry) => entry.path === lastClickedPath);
    return index >= 0 ? index : null;
  }, [entries, lastClickedPath]);

  const handleClick = useCallback(
    (entry: SftpEntry, index: number, additiveKey: boolean, shiftKey: boolean) => {
      setParentSelected(false);
      setSelectedPaths(prev => {
        const next = new Set(prev);

        if (shiftKey && lastClickedIndex !== null) {
          // Desktop file-manager semantics:
          // Shift replaces selection with the anchor range; Ctrl+Shift extends it.
          if (!additiveKey) next.clear();
          const start = Math.min(lastClickedIndex, index);
          const end = Math.max(lastClickedIndex, index);
          for (let i = start; i <= end; i++) {
            if (entries[i]) {
              next.add(entries[i].path);
            }
          }
          // Keep the anchor unchanged for extending the range.
          return next;
        }

        if (additiveKey) {
          // Toggle the clicked entry.
          if (next.has(entry.path)) {
            next.delete(entry.path);
          } else {
            next.add(entry.path);
          }
          setLastClickedPath(entry.path);
          return next;
        }

        // Single select: clear and select only this entry
        next.clear();
        next.add(entry.path);
        setLastClickedPath(entry.path);
        return next;
      });
    },
    [entries, lastClickedIndex]
  );

  const selectAll = useCallback((allEntries: SftpEntry[]) => {
    setParentSelected(false);
    setSelectedPaths(new Set(allEntries.map(e => e.path)));
    setLastClickedPath(null);
  }, []);

  const selectParent = useCallback(() => {
    setParentSelected(true);
    setSelectedPaths(new Set());
    setLastClickedPath(null);
  }, []);

  const clearSelection = useCallback(() => {
    setParentSelected(false);
    setSelectedPaths(new Set());
    setLastClickedPath(null);
  }, []);

  // ── Right-click: auto-select only if not already in selection ──
  //
  // Keep context-menu selection independent of modifier keys. In particular,
  // macOS Control-click is a standard secondary-click gesture and must not be
  // misinterpreted as an additive-selection toggle.
  const handleRightClick = useCallback(
    (entry: SftpEntry) => {
      setParentSelected(false);
      setSelectedPaths(prev => {
        const next = new Set(prev);
        if (!next.has(entry.path)) {
          next.clear();
          next.add(entry.path);
        }
        setLastClickedPath(null);
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
    parentSelected,
    handleClick,
    handleRightClick,
    selectAll,
    selectParent,
    clearSelection,
    isSelected,
    selectionCount: selectedPaths.size,
  }), [
    selectedPaths,
    lastClickedIndex,
    selectedEntries,
    parentSelected,
    handleClick,
    handleRightClick,
    selectAll,
    selectParent,
    clearSelection,
    isSelected,
  ]);
}
