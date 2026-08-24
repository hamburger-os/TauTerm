/**
 * 文件列表组件
 *
 * 带列标题的文件列表，支持排序、多选、加载态、空态和错误横幅。
 * 右键菜单：
 *   - 列标题 / body 空白区域 / 状态文字 → 空白区域菜单
 *   - 文件行 → 文件专用菜单（stopPropagation 阻止冒泡）
 */
import { useRef } from "react";
import { useTranslation } from "react-i18next";
import Icon from "../common/Icon";
import type { SortField, SortDirection } from "./types";
import type { FileViewProps } from "./FileViewProps";
import FileRow from "./FileRow";
import { getFolderIcon } from "./entryIcon";
import styles from "./FileList.module.css";

// ── Helpers ────────────────────────────────────────────

function sortIcon(field: SortField, active: SortField | null, dir: SortDirection): string | null {
  if (field !== active) return null;
  return dir === "asc" ? "chevron-up" : "chevron-down";
}

// ── Component ──────────────────────────────────────────

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
  const bodyRef = useRef<HTMLDivElement>(null);

  // ── 列标题渲染 ──────────────────────────
  const renderHeader = (field: SortField, label: string, extraClass?: string) => (
    <div
      className={`${styles.headerCell} ${extraClass || ""}`}
      onClick={() => onSortChange(field)}
      role="columnheader"
      tabIndex={0}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") onSortChange(field);
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

  // ── 空白区域右键 ──
  // FileRow 已通过 stopPropagation 阻止冒泡，所以到达这里的都是真正的空白区域点击
  const handleBlankContext = (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation(); // 阻止事件冒泡到父级 container/RightSidebarPanel，避免重复触发右键菜单
    onContextMenu(e, null, undefined);
  };

  return (
    <div className={`${styles.container} ${showProgress ? styles.containerWithProgress : ""}`} onContextMenu={handleBlankContext}>
      {/* 列标题 — 右键也触发空白区域菜单 */}
      <div className={styles.header} onContextMenu={handleBlankContext}>
        {renderHeader("name", t("fileManager.name"), styles.colName)}
        {renderHeader("size", t("fileManager.size"), styles.colSize)}
        {renderHeader("modified", t("fileManager.modified"), styles.colTime)}
        <div className={`${styles.headerCell} ${styles.colPerms}`}>
          {t("fileManager.permissions")}
        </div>
      </div>

      {/* 错误横幅 */}
      {error && (
        <div className={styles.errorBanner}>
          <span>{error}</span>
          <button className={styles.errorClose} onClick={onClearError}>
            <Icon name="close" size="xs" />
          </button>
        </div>
      )}

      {/* 文件列表体 */}
      <div
        ref={bodyRef}
        className={styles.body}
        role="grid"
        aria-multiselectable="true"
        onContextMenu={handleBlankContext}
      >
        {/* Parent directory entry */}
        {showParentDir && !loading && (
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
            tabIndex={0}
            onKeyDown={(e) => {
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
        {loading && (
          <div className={styles.status}>{t("fileManager.loading")}</div>
        )}
        {!loading && entries.length === 0 && !error && (
          <div className={styles.status}>{t("fileManager.empty")}</div>
        )}
        {!loading &&
          entries.map((entry, index) => (
            <FileRow
              key={entry.path}
              entry={entry}
              isSelected={selectedPaths.has(entry.path)}
              onClick={(e) =>
                onEntryClick(entry, index, e.ctrlKey, e.shiftKey)
              }
              onDoubleClick={() => onEntryDoubleClick(entry)}
              onContextMenu={(e) => {
                e.preventDefault();
                e.stopPropagation();
                onContextMenu(e, entry, index);
              }}
            />
          ))}
      </div>
    </div>
  );
}
