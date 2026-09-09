/**
 * 文件网格组件（系统平铺式）
 *
 * 图标在左、右侧上下排列文件名 / 类型·大小。
 * 大目录按可视行 windowing；键盘使用 roving tabindex + 方向键导航。
 */
import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { formatBytes } from "../../utils/format";
import type { SftpEntry } from "./types";
import type { FileViewProps } from "./FileViewProps";
import { getEntryCategory, getEntryIcon, getFolderIcon, CATEGORY_LABEL_KEYS } from "./entryIcon";
import styles from "./FileGrid.module.css";

type FileGridProps = FileViewProps;

const GRID_MIN_COLUMN = 160;
const GRID_GAP = 4;
const GRID_ROW_HEIGHT = 48; // 44px tile + 4px (--spacing-xs) row gap
const VIRTUAL_THRESHOLD = 300;
const OVERSCAN_ROWS = 4;

interface FileTileProps {
  entry: SftpEntry;
  isSelected: boolean;
  typeLabel: string;
  tabIndex: number;
  dataIndex: number;
  onFocus: () => void;
  onKeyDown: (e: React.KeyboardEvent) => void;
  onClick: (e: React.MouseEvent) => void;
  onDoubleClick: () => void;
  onContextMenu: (e: React.MouseEvent) => void;
}

const FileTile = memo(function FileTile({
  entry,
  isSelected,
  typeLabel,
  tabIndex,
  dataIndex,
  onFocus,
  onKeyDown,
  onClick,
  onDoubleClick,
  onContextMenu,
}: FileTileProps) {
  const tileClass = [styles.tile, isSelected ? styles.selected : ""]
    .filter(Boolean)
    .join(" ");

  return (
    <div
      className={tileClass}
      onClick={onClick}
      onDoubleClick={onDoubleClick}
      onContextMenu={onContextMenu}
      role="row"
      aria-selected={isSelected}
      tabIndex={tabIndex}
      data-grid-index={dataIndex}
      onFocus={onFocus}
      onKeyDown={(e) => {
        onKeyDown(e);
        if (e.defaultPrevented) return;
        if (e.key === "Enter") {
          e.preventDefault();
          onDoubleClick();
        } else if (e.key === " ") {
          e.preventDefault();
          onClick({ ctrlKey: false, shiftKey: false } as React.MouseEvent);
        }
      }}
    >
      <span className={styles.tileIcon} role="gridcell">{getEntryIcon(entry)}</span>
      <div className={styles.tileMeta} role="gridcell">
        <span className={styles.tileName} title={entry.name}>
          {entry.name}
        </span>
        <span className={styles.tileSub}>
          {entry.is_dir ? typeLabel : `${typeLabel} · ${formatBytes(entry.size)}`}
        </span>
      </div>
    </div>
  );
});

export default function FileGrid({
  entries,
  loading,
  error,
  selectedPaths,
  onEntryClick,
  onEntryDoubleClick,
  onContextMenu,
  onClearError,
  showParentDir,
  onGoUp,
  parentSelected,
  onParentClick,
  showProgress = false,
}: FileGridProps) {
  const { t } = useTranslation();
  const bodyRef = useRef<HTMLDivElement>(null);
  const [viewport, setViewport] = useState({ width: 0, height: 0 });
  const [scrollTop, setScrollTop] = useState(0);
  const parentVisible = showParentDir && !loading;
  const itemCount = entries.length + (parentVisible ? 1 : 0);
  const [activeItem, setActiveItem] = useState(0);

  useEffect(() => {
    const node = bodyRef.current;
    if (!node) return;
    const update = () => setViewport({ width: node.clientWidth, height: node.clientHeight });
    update();
    const observer = new ResizeObserver(update);
    observer.observe(node);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    setActiveItem((current) => Math.min(Math.max(current, 0), Math.max(0, itemCount - 1)));
  }, [itemCount]);

  const columns = Math.max(
    1,
    Math.floor((Math.max(viewport.width, GRID_MIN_COLUMN) + GRID_GAP) / (GRID_MIN_COLUMN + GRID_GAP)),
  );
  const totalRows = Math.ceil(itemCount / columns);
  const virtualEnabled = itemCount >= VIRTUAL_THRESHOLD;
  const startRow = virtualEnabled
    ? Math.max(0, Math.floor(scrollTop / GRID_ROW_HEIGHT) - OVERSCAN_ROWS)
    : 0;
  const visibleRows = Math.ceil(Math.max(viewport.height, GRID_ROW_HEIGHT) / GRID_ROW_HEIGHT);
  const endRow = virtualEnabled
    ? Math.min(totalRows, startRow + visibleRows + OVERSCAN_ROWS * 2)
    : totalRows;
  const startItem = startRow * columns;
  const endItem = Math.min(itemCount, endRow * columns);

  const allItems = useMemo<Array<SftpEntry | null>>(
    () => parentVisible ? [null, ...entries] : entries,
    [entries, parentVisible],
  );

  const focusItem = useCallback((next: number) => {
    const clamped = Math.min(Math.max(next, 0), Math.max(0, itemCount - 1));
    setActiveItem(clamped);

    if (virtualEnabled && bodyRef.current) {
      const row = Math.floor(clamped / columns);
      const top = row * GRID_ROW_HEIGHT;
      const bottom = top + GRID_ROW_HEIGHT;
      if (top < bodyRef.current.scrollTop) {
        bodyRef.current.scrollTop = top;
      } else if (bottom > bodyRef.current.scrollTop + bodyRef.current.clientHeight) {
        bodyRef.current.scrollTop = bottom - bodyRef.current.clientHeight;
      }
      setScrollTop(bodyRef.current.scrollTop);
    }

    requestAnimationFrame(() => {
      bodyRef.current
        ?.querySelector<HTMLElement>(`[data-grid-index="${clamped}"]`)
        ?.focus();
    });
  }, [columns, itemCount, virtualEnabled]);

  const handleNavigationKey = useCallback((current: number, e: React.KeyboardEvent) => {
    if (itemCount === 0) return;
    let next = current;
    switch (e.key) {
      case "ArrowLeft":
        next = current - 1;
        break;
      case "ArrowRight":
        next = current + 1;
        break;
      case "ArrowUp":
        next = current - columns;
        break;
      case "ArrowDown":
        next = current + columns;
        break;
      case "Home":
        next = 0;
        break;
      case "End":
        next = itemCount - 1;
        break;
      case "PageUp":
        next = current - columns * 5;
        break;
      case "PageDown":
        next = current + columns * 5;
        break;
      default:
        return;
    }
    e.preventDefault();
    focusItem(next);
  }, [columns, focusItem, itemCount]);

  const handleBlankContext = (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    onContextMenu(e, null, undefined);
  };

  const renderItem = (item: SftpEntry | null, itemIndex: number) => {
    if (item === null) {
      return (
        <div
          key="__parent__"
          className={`${styles.tile} ${parentSelected ? styles.selected : ""}`}
          onClick={onParentClick}
          onDoubleClick={onGoUp}
          onContextMenu={(e) => {
            e.preventDefault();
            e.stopPropagation();
            onContextMenu(e, null, undefined);
          }}
          role="row"
          aria-selected={parentSelected}
          tabIndex={activeItem === itemIndex ? 0 : -1}
          data-grid-index={itemIndex}
          onFocus={() => setActiveItem(itemIndex)}
          onKeyDown={(e) => {
            handleNavigationKey(itemIndex, e);
            if (e.defaultPrevented) return;
            if (e.key === "Enter") {
              e.preventDefault();
              onGoUp();
            } else if (e.key === " ") {
              e.preventDefault();
              onParentClick();
            }
          }}
        >
          <span className={styles.tileIcon} role="gridcell">{getFolderIcon()}</span>
          <div className={styles.tileMeta} role="gridcell">
            <span className={styles.tileName}>..</span>
            <span className={styles.tileSub}>{t("fileManager.parentDir")}</span>
          </div>
        </div>
      );
    }

    const entryIndex = itemIndex - (parentVisible ? 1 : 0);
    return (
      <FileTile
        key={item.path}
        entry={item}
        isSelected={selectedPaths.has(item.path)}
        typeLabel={t(CATEGORY_LABEL_KEYS[getEntryCategory(item)])}
        tabIndex={activeItem === itemIndex ? 0 : -1}
        dataIndex={itemIndex}
        onFocus={() => setActiveItem(itemIndex)}
        onKeyDown={(e) => handleNavigationKey(itemIndex, e)}
        onClick={(e) => onEntryClick(item, entryIndex, e.ctrlKey, e.shiftKey)}
        onDoubleClick={() => onEntryDoubleClick(item)}
        onContextMenu={(e) => {
          e.preventDefault();
          e.stopPropagation();
          onContextMenu(e, item, entryIndex);
        }}
      />
    );
  };

  const visibleItems = virtualEnabled
    ? allItems.slice(startItem, endItem)
    : allItems;

  return (
    <div
      className={`${styles.container} ${showProgress ? styles.containerWithProgress : ""}`}
      onContextMenu={handleBlankContext}
    >
      {error && (
        <div className={styles.errorBanner}>
          <span>{error}</span>
          <button
            className={styles.errorClose}
            onClick={onClearError}
            aria-label={t("common.close")}
          >
            ×
          </button>
        </div>
      )}

      <div
        ref={bodyRef}
        className={`${styles.body} ${virtualEnabled ? styles.virtualBody : ""}`}
        role="grid"
        aria-multiselectable="true"
        aria-rowcount={itemCount}
        onContextMenu={handleBlankContext}
        onScroll={(e) => {
          if (virtualEnabled) setScrollTop(e.currentTarget.scrollTop);
        }}
      >
        {loading && <div className={styles.status}>{t("fileManager.loading")}</div>}
        {!loading && entries.length === 0 && !error && !parentVisible && (
          <div className={styles.status}>{t("fileManager.empty")}</div>
        )}

        {!loading && itemCount > 0 && (
          virtualEnabled ? (
            <div
              className={styles.virtualCanvas}
              style={{ height: totalRows * GRID_ROW_HEIGHT }}
              role="presentation"
            >
              <div
                className={styles.virtualSlice}
                style={{
                  top: startRow * GRID_ROW_HEIGHT,
                  gridTemplateColumns: `repeat(${columns}, minmax(0, 1fr))`,
                }}
                role="presentation"
              >
                {visibleItems.map((item, offset) => renderItem(item, startItem + offset))}
              </div>
            </div>
          ) : (
            visibleItems.map((item, index) => renderItem(item, index))
          )
        )}
      </div>
    </div>
  );
}
