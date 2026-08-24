/**
 * 文件行组件
 *
 * 文件列表中的单行，展示图标、文件名、大小、修改时间和权限。
 * 支持单选高亮、右键菜单。
 * 使用 React.memo 避免选择变化时全部行重渲染。
 */
import { memo } from "react";
import type { SftpEntry } from "./types";
import { formatBytes, formatTime } from "../../utils/format";
import { getEntryIcon } from "./entryIcon";
import styles from "./FileRow.module.css";

// ── Component ──────────────────────────────────────────

interface FileRowProps {
  entry: SftpEntry;
  isSelected: boolean;
  onClick: (e: React.MouseEvent) => void;
  onDoubleClick: () => void;
  onContextMenu: (e: React.MouseEvent) => void;
}

const FileRow = memo(function FileRow({
  entry,
  isSelected,
  onClick,
  onDoubleClick,
  onContextMenu,
}: FileRowProps) {
  const rowClass = [
    styles.row,
    isSelected ? styles.selected : "",
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <div
      className={rowClass}
      onClick={onClick}
      onDoubleClick={onDoubleClick}
      onContextMenu={onContextMenu}
      role="row"
      aria-selected={isSelected}
      tabIndex={0}
      onKeyDown={(e) => {
        if (e.key === "Enter") {
          e.preventDefault();
          onDoubleClick();
        } else if (e.key === " ") {
          e.preventDefault();
          onClick({ ctrlKey: false, shiftKey: false } as React.MouseEvent);
        }
      }}
    >
      <span className={styles.icon} role="gridcell">{getEntryIcon(entry)}</span>
      <span className={styles.name} role="gridcell">{entry.name}</span>
      <span className={styles.size} role="gridcell">
        {entry.is_dir ? "-" : formatBytes(entry.size)}
      </span>
      <span className={styles.time} role="gridcell">{formatTime(entry.modified)}</span>
      <span className={styles.perms} role="gridcell">{entry.permissions || "-"}</span>
    </div>
  );
});

export default FileRow;
