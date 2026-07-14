import { useState, useRef, useCallback } from "react";

/**
 * usePointerDragReorder — 通用的指针事件拖拽排序 hook
 * General-purpose pointer-event-based drag-and-drop reorder hook.
 *
 * 使用 Pointer Events API 实现列表项拖拽排序，兼容 Tauri WebView2 环境。
 * 自动管理 pointer capture、drop 指示器状态以及拖拽过程中的视觉反馈。
 *
 * Uses the Pointer Events API for list reordering, compatible with Tauri
 * WebView2. Manages pointer capture, drop-indicator state, and visual
 * feedback during drag operations.
 *
 * @template T — 列表项类型 / list item type
 * @param items     — 当前列表数据 / current list data
 * @param onReorder — 排序完成后的回调，接收重排后的新数组 /
 *                    callback invoked after reorder with the new array
 * @param options   — 配置项 / configuration
 * @param options.itemSelector  — CSS 选择器，用于定位可拖拽行元素 /
 *                                CSS selector to locate draggable row elements
 * @param options.draggingClass — 拖拽中施加于行元素的 CSS class 名 /
 *                                CSS class name applied to the row while dragging
 * @param options.disabled      — 是否禁用拖拽（如运行中） /
 *                                whether dragging is disabled (e.g. while running)
 * @param options.listRef       — 列表容器元素的 ref /
 *                                ref to the list container element
 *
 * @returns 拖拽状态与事件处理器 / drag state and event handlers
 */
export function usePointerDragReorder<T>(
  items: T[],
  onReorder: (items: T[]) => void,
  options: {
    itemSelector: string;
    draggingClass: string;
    disabled?: boolean;
    listRef: React.RefObject<HTMLDivElement | null>;
  },
): {
  isDragging: boolean;
  dropIndex: number | null;
  handlePointerDown: (e: React.PointerEvent, index: number) => void;
  handlePointerMove: (e: React.PointerEvent) => void;
  handlePointerUp: (e: React.PointerEvent) => void;
  handlePointerCancel: (e: React.PointerEvent) => void;
} {
  const { itemSelector, draggingClass, disabled, listRef } = options;

  // ── 拖拽索引使用 ref 存储，避免频繁重渲染 ──
  // drag index stored as ref to avoid excessive re-renders
  const dragIndexRef = useRef<number | null>(null);
  const dropIndexRef = useRef<number | null>(null);

  const [isDragging, setIsDragging] = useState(false);
  const [dropIndex, setDropIndex] = useState<number | null>(null);

  /**
   * 根据指针 Y 坐标计算目标插入位置
   * Calculates the target insertion index from the pointer's Y coordinate.
   *
   * 遍历所有可拖拽行，比较指针 Y 与每行中线的位置：
   * - 若指针在行中线以上，返回该行索引
   * - 若指针在所有行以下，返回 rows.length（末尾）
   *
   * Iterates draggable rows and compares pointer Y against each row's
   * vertical midpoint. Returns the row index if pointer is above the
   * midpoint, or rows.length if pointer is below all rows.
   */
  const calcDropIndex = useCallback(
    (clientY: number): number => {
      const rows = listRef.current?.querySelectorAll<HTMLElement>(itemSelector);
      if (!rows || rows.length === 0) return 0;
      for (let i = 0; i < rows.length; i++) {
        const rect = rows[i].getBoundingClientRect();
        const midY = rect.top + rect.height / 2;
        if (clientY < midY) return i;
      }
      return rows.length;
    },
    [listRef, itemSelector],
  );

  /**
   * 指针按下：启动拖拽，捕获指针，记录起始索引
   * Pointer down: initiates drag, captures pointer, records start index.
   *
   * 仅响应主指针（isPrimary），若 disabled 则忽略。
   * 对拖拽手柄调用 setPointerCapture 以接收后续移动/释放事件。
   * 将 draggingClass 施加到最近的 itemSelector 行上作为视觉反馈。
   *
   * Only responds to primary pointer; ignored when disabled.
   * Calls setPointerCapture on the drag handle to receive subsequent
   * move/release events. Applies draggingClass to the closest item row
   * for visual feedback.
   */
  const handlePointerDown = useCallback(
    (e: React.PointerEvent, index: number) => {
      if (disabled || !e.isPrimary) return;
      e.preventDefault();
      const handle = e.currentTarget as HTMLElement;
      handle.setPointerCapture(e.pointerId);
      dragIndexRef.current = index;
      setIsDragging(true);
      const row = handle.closest(itemSelector);
      if (row) row.classList.add(draggingClass);
      dropIndexRef.current = index;
      setDropIndex(index);
    },
    [disabled, itemSelector, draggingClass],
  );

  /**
   * 指针移动：更新 drop 指示器位置
   * Pointer move: updates the drop indicator position.
   *
   * 仅在拖拽进行中时生效。调用 calcDropIndex 计算当前指针位置对应的
   * 目标索引，仅在索引变化时更新状态以最小化重渲染。
   *
   * Only active while dragging. Uses calcDropIndex to determine the
   * target index from the current pointer position. State is updated
   * only when the index actually changes, minimizing re-renders.
   */
  const handlePointerMove = useCallback(
    (e: React.PointerEvent) => {
      if (dragIndexRef.current === null) return;
      e.preventDefault();
      const newIndex = calcDropIndex(e.clientY);
      if (newIndex !== dropIndexRef.current) {
        dropIndexRef.current = newIndex;
        setDropIndex(newIndex);
      }
    },
    [calcDropIndex],
  );

  /**
   * 指针释放：完成排序，释放指针捕获，清理状态
   * Pointer up: finalizes reorder, releases pointer capture, cleans up.
   *
   * 根据起始索引和目标索引计算调整后的插入位置（移除原项后索引可能偏移），
   * 若调整后的位置与起始位置不同则执行 splice 重排并调用 onReorder。
   * 移除 draggingClass 并重置所有拖拽状态。
   *
   * Computes the adjusted insertion index accounting for the removed
   * item's offset. If the adjusted index differs from the source index,
   * performs a splice-based reorder and invokes onReorder. Removes the
   * draggingClass and resets all drag state.
   */
  const handlePointerUp = useCallback(
    (e: React.PointerEvent) => {
      const fromIndex = dragIndexRef.current;
      if (fromIndex === null) return;
      const handle = e.currentTarget as HTMLElement;
      handle.releasePointerCapture(e.pointerId);
      const row = handle.closest(itemSelector);
      if (row) row.classList.remove(draggingClass);

      const targetIndex = dropIndexRef.current;
      if (targetIndex !== null && targetIndex !== fromIndex) {
        // 当 fromIndex < targetIndex 时，移除原项后目标索引会前移一位
        // When fromIndex < targetIndex, the target shifts back by 1
        // after removing the original item.
        const adjustedIndex =
          fromIndex < targetIndex ? targetIndex - 1 : targetIndex;
        if (fromIndex !== adjustedIndex) {
          const next = [...items];
          const [removed] = next.splice(fromIndex, 1);
          next.splice(adjustedIndex, 0, removed);
          onReorder(next);
        }
      }

      dragIndexRef.current = null;
      dropIndexRef.current = null;
      setDropIndex(null);
      setIsDragging(false);
    },
    [items, onReorder, itemSelector, draggingClass],
  );

  /**
   * 指针取消：清理拖拽状态（如右键点击、窗口失焦等导致的中断）
   * Pointer cancel: cleans up drag state on interruption (e.g. right-click,
   * window blur, etc.).
   *
   * 若当前无拖拽则直接返回。释放指针捕获（容错已释放的情况），
   * 移除 draggingClass 并重置所有状态。
   *
   * No-op if not actively dragging. Releases pointer capture (tolerating
   * already-released), removes draggingClass, and resets all state.
   */
  const handlePointerCancel = useCallback(
    (e: React.PointerEvent) => {
      if (dragIndexRef.current === null) return;
      const handle = e.currentTarget as HTMLElement;
      try {
        handle.releasePointerCapture(e.pointerId);
      } catch {
        /* pointer capture already released */
      }
      const row = handle.closest(itemSelector);
      if (row) row.classList.remove(draggingClass);
      dragIndexRef.current = null;
      dropIndexRef.current = null;
      setDropIndex(null);
      setIsDragging(false);
    },
    [itemSelector, draggingClass],
  );

  return {
    isDragging,
    dropIndex,
    handlePointerDown,
    handlePointerMove,
    handlePointerUp,
    handlePointerCancel,
  };
}
