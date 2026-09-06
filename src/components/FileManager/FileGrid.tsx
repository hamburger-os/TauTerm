/**
 * 文件网格组件（系统平铺式）
 *
 * 图标在左、右侧上下排列 文件名 / 类型·大小。
 * 与 FileList 共享选择、双击、右键菜单交互；不支持列排序（排序沿用列表状态）。
 */
import { memo } from "react";
import { useTranslation } from "react-i18next";
import { formatBytes } from "../../utils/format";
import type { SftpEntry } from "./types";
import type { FileViewProps } from "./FileViewProps";
import { getEntryCategory, getEntryIcon, getFolderIcon, CATEGORY_LABEL_KEYS } from "./entryIcon";
import styles from "./FileGrid.module.css";

type FileGridProps = FileViewProps;

interface FileTileProps {
  entry: SftpEntry;
  isSelected: boolean;
  typeLabel: string;
  onClick: (e: React.MouseEvent) => void;
  onDoubleClick: () => void;
  onContextMenu: (e: React.MouseEvent) => void;
}

const FileTile = memo(function FileTile({
  entry,
  isSelected,
  typeLabel,
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

  const handleBlankContext = (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation(); // 阻止事件冒泡到父级 container/RightSidebarPanel，避免重复触发右键菜单
    onContextMenu(e, null, undefined);
  };

  return (
    <div
      className={`${styles.container} ${showProgress ? styles.containerWithProgress : ""}`}
      onContextMenu={handleBlankContext}
    >
      {/* 错误横幅 */}
      {error && (
        <div className={styles.errorBanner}>
          <span>{error}</span>
          <button className={styles.errorClose} onClick={onClearError}>
            ×
          </button>
        </div>
      )}

      {/* 网格体 */}
      <div
        className={styles.body}
        role="grid"
        aria-multiselectable="true"
        onContextMenu={handleBlankContext}
      >
        {showParentDir && !loading && (
          <div
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
            <span className={styles.tileIcon} role="gridcell">{getFolderIcon()}</span>
            <div className={styles.tileMeta} role="gridcell">
              <span className={styles.tileName}>..</span>
              <span className={styles.tileSub}>{t("fileManager.parentDir")}</span>
            </div>
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
            <FileTile
              key={entry.path}
              entry={entry}
              isSelected={selectedPaths.has(entry.path)}
              typeLabel={t(CATEGORY_LABEL_KEYS[getEntryCategory(entry)])}
              onClick={(e) => onEntryClick(entry, index, e.ctrlKey, e.shiftKey)}
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
