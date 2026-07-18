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
      tabIndex={0}
      onKeyDown={(e) => {
        if (e.key === "Enter") onDoubleClick();
      }}
    >
      <span className={styles.icon}>
        {entry.is_dir ? "\u{1F4C1}" : "\u{1F4C4}"}
      </span>
      <span className={styles.name}>{entry.name}</span>
      <span className={styles.size}>
        {entry.is_dir ? "-" : formatBytes(entry.size)}
      </span>
      <span className={styles.time}>{formatTime(entry.modified)}</span>
      <span className={styles.perms}>{entry.permissions || "-"}</span>
    </div>
  );
});

export default FileRow;
