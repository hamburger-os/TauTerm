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
import type { SftpEntry, SortField, SortDirection } from "./types";
import FileRow from "./FileRow";
import styles from "./FileList.module.css";

// ── Helpers ────────────────────────────────────────────

function sortArrow(field: SortField, active: SortField | null, dir: SortDirection): string {
  if (field !== active) return "";
  return dir === "asc" ? "▲" : "▼";
}

// ── Component ──────────────────────────────────────────

interface FileListProps {
  entries: SftpEntry[];
  loading: boolean;
  error: string | null;
  selectedPaths: Set<string>;
  sortField: SortField;
  sortDirection: SortDirection;
  onSortChange: (field: SortField) => void;
  onEntryClick: (
    entry: SftpEntry,
    index: number,
    ctrlKey: boolean,
    shiftKey: boolean
  ) => void;
  onEntryDoubleClick: (entry: SftpEntry) => void;
  onContextMenu: (e: React.MouseEvent, entry: SftpEntry | null, index?: number) => void;
  onClearError: () => void;
  showParentDir: boolean;
  onGoUp: () => void;
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
      <span className={styles.sortArrow}>
        {sortArrow(field, sortField, sortDirection)}
      </span>
    </div>
  );

  // ── 空白区域右键 ──
  // FileRow 已通过 stopPropagation 阻止冒泡，所以到达这里的都是真正的空白区域点击
  const handleBlankContext = (e: React.MouseEvent) => {
    e.preventDefault();
    onContextMenu(e, null, undefined);
  };

  return (
    <div className={styles.container} onContextMenu={handleBlankContext}>
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
            X
          </button>
        </div>
      )}

      {/* 文件列表体 */}
      <div ref={bodyRef} className={styles.body} onContextMenu={handleBlankContext}>
        {/* Parent directory entry */}
        {showParentDir && !loading && (
          <div
            className={styles.parentDirRow}
            onClick={onGoUp}
            onDoubleClick={onGoUp}
            onContextMenu={(e) => {
              e.preventDefault();
              e.stopPropagation();
              onContextMenu(e, null, undefined);
            }}
            role="row"
            tabIndex={0}
            onKeyDown={(e) => {
              if (e.key === "Enter") onGoUp();
            }}
          >
            <span className={styles.parentDirIcon}>📁</span>
            <span className={styles.parentDirName}>..</span>
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
