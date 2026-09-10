/**
 * 文件列表组件
 *
 * 带列标题的文件列表，支持排序、多选、加载态、空态、错误横幅。
 * 大目录启用内建 windowing，仅渲染可视区附近行；键盘焦点采用 roving tabindex。
 */
import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import Icon from "../common/Icon";
import type { IconName } from "../common/Icon";
import type { SortField, SortDirection } from "./types";
import type { FileViewProps } from "./FileViewProps";
import FileRow from "./FileRow";
import { getFolderIcon } from "./entryIcon";
import { useVirtualWindow } from "./hooks/useVirtualWindow";
import styles from "./FileList.module.css";

const ROW_HEIGHT = 28;
const PAGE_STEP = 10;

function sortIcon(field: SortField, active: SortField | null, dir: SortDirection): IconName | null {
  if (field !== active) return null;
  return dir === "asc" ? "arrow-up" : "arrow-down";
}

interface FileListProps extends FileViewProps {
  sortField: SortField;
  sortDirection: SortDirection;
  onSortChange: (field: SortField) => void;
}

export default function FileList({
  entries,
  loading,
  error,
  selectedPaths,
  sortField,
  sortDirection,
  onSortChange,
  onEntryClick,
  onEntryDoubleClick,
  onContextMenu,
  onClearError,
  showParentDir,
  onGoUp,
  parentSelected,
  onParentClick,
  showProgress = false,
}: FileListProps) {
  const { t } = useTranslation();
  const parentVisible = showParentDir && !loading;
  const [activeIndex, setActiveIndex] = useState<number>(parentVisible ? -1 : 0);
  const virtual = useVirtualWindow({
    count: entries.length,
    itemSize: ROW_HEIGHT,
    leadingSize: parentVisible ? ROW_HEIGHT : 0,
  });

  useEffect(() => {
    if (loading) return;
    if (entries.length === 0) {
      setActiveIndex(parentVisible ? -1 : 0);
      return;
    }
    setActiveIndex((current) => {
      if (current === -1 && parentVisible) return -1;
      return Math.min(Math.max(current, 0), entries.length - 1);
    });
  }, [entries.length, loading, parentVisible]);

  const focusIndex = useCallback((index: number) => {
    if (index >= 0) virtual.scrollIndexIntoView(index);
    setActiveIndex(index);
    requestAnimationFrame(() => {
      const selector = index === -1
        ? '[data-file-index="parent"]'
        : `[data-file-index="${index}"]`;
      const target = virtual.containerRef.current?.querySelector<HTMLElement>(selector);
      target?.focus();
    });
  }, [virtual.containerRef, virtual.scrollIndexIntoView]);

  const handleNavigationKey = useCallback((current: number, e: React.KeyboardEvent) => {
    if (entries.length === 0 && !parentVisible) return;

    let next = current;
    switch (e.key) {
      case "ArrowDown":
        next = current < 0 ? 0 : Math.min(entries.length - 1, current + 1);
        break;
      case "ArrowUp":
        next = current <= 0 && parentVisible
          ? -1
          : Math.max(0, current - 1);
        break;
      case "Home":
        next = parentVisible ? -1 : 0;
        break;
      case "End":
        next = Math.max(0, entries.length - 1);
        break;
      case "PageDown":
        next = Math.min(entries.length - 1, Math.max(0, current) + PAGE_STEP);
        break;
      case "PageUp":
        next = Math.max(parentVisible ? -1 : 0, current - PAGE_STEP);
        break;
      default:
        return;
    }

    e.preventDefault();
    focusIndex(next);
  }, [entries.length, focusIndex, parentVisible]);

  const renderHeader = (field: SortField, label: string, extraClass?: string) => (
    <div
      className={`${styles.headerCell} ${extraClass || ""}`}
      onClick={() => onSortChange(field)}
      role="columnheader"
      aria-sort={
        field === sortField
          ? sortDirection === "asc"
            ? "ascending"
            : "descending"
          : "none"
      }
      tabIndex={0}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onSortChange(field);
        }
      }}
    >
      {label}
      {sortIcon(field, sortField, sortDirection) && (
        <span className={styles.sortArrow}>
          <Icon name={sortIcon(field, sortField, sortDirection)!} size="xs" />
        </span>
      )}
    </div>
  );

  const handleBlankContext = (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    onContextMenu(e, null, undefined);
  };

  const renderRow = (entry: FileViewProps["entries"][number], index: number, virtualized: boolean) => (
    <FileRow
      key={entry.path}
      entry={entry}
      isSelected={selectedPaths.has(entry.path)}
      tabIndex={activeIndex === index ? 0 : -1}
      dataIndex={index}
      style={virtualized
        ? { position: "absolute", top: index * ROW_HEIGHT, left: 0, right: 0 }
        : undefined}
      onFocus={() => setActiveIndex(index)}
      onKeyDown={(e) => handleNavigationKey(index, e)}
      onClick={(additiveKey, shiftKey) => onEntryClick(entry, index, additiveKey, shiftKey)}
      onDoubleClick={() => onEntryDoubleClick(entry)}
      onContextMenu={(e) => {
        e.preventDefault();
        e.stopPropagation();
        onContextMenu(e, entry, index);
      }}
    />
  );

  return (
    <div
      className={`${styles.container} ${showProgress ? styles.containerWithProgress : ""}`}
      onContextMenu={handleBlankContext}
    >
      <div className={styles.header} onContextMenu={handleBlankContext}>
        {renderHeader("name", t("fileManager.name"), styles.colName)}
        {renderHeader("size", t("fileManager.size"), styles.colSize)}
        {renderHeader("modified", t("fileManager.modified"), styles.colTime)}
        <div className={`${styles.headerCell} ${styles.colPerms}`} role="columnheader">
          {t("fileManager.permissions")}
        </div>
      </div>

      {error && (
        <div className={styles.errorBanner} role="alert">
          <span>{error}</span>
          <button
            type="button"
            className={`${styles.errorClose} liquid-glass-ghost-button`}
            onClick={onClearError}
            aria-label={t("common.close")}
          >
            <Icon name="close" size="xs" />
          </button>
        </div>
      )}

      <div
        ref={virtual.containerRef}
        className={styles.body}
        role="grid"
        aria-multiselectable="true"
        aria-rowcount={entries.length + (parentVisible ? 1 : 0)}
        onContextMenu={handleBlankContext}
        onScroll={virtual.onScroll}
      >
        {parentVisible && (
          <div
            className={`${styles.parentDirRow} ${parentSelected ? styles.parentDirSelected : ""}`}
            onClick={onParentClick}
            onDoubleClick={onGoUp}
            onContextMenu={(e) => {
              e.preventDefault();
              e.stopPropagation();
              onContextMenu(e, null, undefined);
            }}
            role="row"
            aria-selected={parentSelected}
            tabIndex={activeIndex === -1 ? 0 : -1}
            data-file-index="parent"
            onFocus={() => setActiveIndex(-1)}
            onKeyDown={(e) => {
              handleNavigationKey(-1, e);
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
            <span className={styles.parentDirIcon} role="gridcell">{getFolderIcon()}</span>
            <span className={styles.parentDirName} role="gridcell">..</span>
          </div>
        )}

        {loading && <div className={styles.status}>{t("fileManager.loading")}</div>}
        {!loading && entries.length === 0 && !error && (
          <div className={styles.status}>{t("fileManager.empty")}</div>
        )}

        {!loading && entries.length > 0 && (
          virtual.enabled ? (
            <div
              className={styles.virtualCanvas}
              style={{ height: virtual.totalSize }}
              role="presentation"
            >
              {entries
                .slice(virtual.start, virtual.end)
                .map((entry, offset) => renderRow(entry, virtual.start + offset, true))}
            </div>
          ) : (
            entries.map((entry, index) => renderRow(entry, index, false))
          )
        )}
      </div>
    </div>
  );
}
