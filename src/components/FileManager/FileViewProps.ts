import type { SftpEntry } from "./types";

/**
 * FileList 与 FileGrid 共有的 props。
 * 视图切换下两者共享同一套选择/双击/右键/错误处理契约，仅渲染形态与排序能力不同。
 */
export interface FileViewProps {
  entries: SftpEntry[];
  loading: boolean;
  error: string | null;
  selectedPaths: Set<string>;
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
  /** 上级目录入口是否处于选中（高亮）态 */
  parentSelected: boolean;
  /** 单击上级目录入口（选中，而非返回） */
  onParentClick: () => void;
  /** 进度条可见时，容器底部预留空间避免遮挡列表/网格 */
  showProgress?: boolean;
}
