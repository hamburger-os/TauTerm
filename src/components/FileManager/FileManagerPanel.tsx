/**
 * 文件管理器面板
 *
 * 在右侧栏显示 SFTP 远程文件浏览器。
 * 支持目录浏览、上传、下载、删除、重命名、新建文件/文件夹、
 * 多选批量操作、右键菜单、快捷键、传输进度条。
 */
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import type { SftpEntry } from "./types";
import { useFileManager } from "./hooks/useFileManager";
import { useMultiSelect } from "./hooks/useMultiSelect";
import { useSftpProgress } from "./hooks/useSftpProgress";
import BreadcrumbNav from "./BreadcrumbNav";
import FileList from "./FileList";
import InlinePrompt from "./InlinePrompt";
import CommonContextMenu, { type ContextMenuItem } from "../common/ContextMenu";
import TransferProgressBar from "./TransferProgressBar";
import FilePropertiesModal from "./FilePropertiesModal";
import type { FileStatInfo } from "./FilePropertiesModal";
import FilePreviewModal from "./FilePreviewModal";
import styles from "./FileManager.module.css";

// ── 文本文件扩展名判定 ─────────────────────────────────

const TEXT_EXTENSIONS = new Set([
  ".txt", ".log", ".cfg", ".conf", ".ini", ".json", ".xml", ".yaml", ".yml",
  ".toml", ".sh", ".bash", ".zsh", ".py", ".rb", ".js", ".ts", ".jsx", ".tsx",
  ".css", ".html", ".md", ".c", ".cpp", ".h", ".hpp", ".rs", ".go", ".java",
  ".lua", ".service", ".env", ".gitignore", ".editorconfig",
]);

function isTextFile(name: string): boolean {
  const dot = name.lastIndexOf(".");
  if (dot === -1) return false;
  return TEXT_EXTENSIONS.has(name.slice(dot).toLowerCase());
}

interface FileManagerPanelProps {
  sessionId: string;
  isConnected: boolean;
}

export default function FileManagerPanel({
  sessionId,
  isConnected,
}: FileManagerPanelProps) {
  const { t } = useTranslation();
  const panelRef = useRef<HTMLDivElement>(null);

  // ── Core file manager state ────────────────────────
  const fm = useFileManager(sessionId, isConnected);

  // ── Multi-select ────────────────────────────────────
  const ms = useMultiSelect(fm.entries);

  // ── Progress events ─────────────────────────────────
  const { progress, hideProgress, cancelTransfer } = useSftpProgress(sessionId);

  // ── Drag-drop target tracking (for App.tsx routing) ──
  const [isDragOver, setIsDragOver] = useState(false);
  const dragCounterRef = useRef(0);

  // 同步当前路径到全局，供 App.tsx 的 SFTP 拖放上传使用
  useEffect(() => {
    window.__tauterm_filemanagerPath = fm.currentPath;
  }, [fm.currentPath]);

  // 监听 App.tsx SFTP 上传完成后的刷新事件
  useEffect(() => {
    const handleRefresh = () => {
      fm.refresh();
    };
    window.addEventListener("tauterm:sftp-refresh", handleRefresh);
    return () => window.removeEventListener("tauterm:sftp-refresh", handleRefresh);
  }, [fm]);

  // 注册 DOM 拖放事件 — 标记拖放目标为 filemanager，供 App.tsx 区分路由
  useEffect(() => {
    const el = panelRef.current;
    if (!el) return;

    const handleDragEnter = (e: DragEvent) => {
      e.preventDefault();
      e.stopPropagation();
      dragCounterRef.current += 1;
      if (dragCounterRef.current === 1) {
        window.__tauterm_dropTarget = "filemanager";
        setIsDragOver(true);
      }
    };

    const handleDragLeave = (_e: DragEvent) => {
      dragCounterRef.current -= 1;
      if (dragCounterRef.current <= 0) {
        dragCounterRef.current = 0;
        window.__tauterm_dropTarget = undefined;
        setIsDragOver(false);
      }
    };

    const handleDragOver = (e: DragEvent) => {
      e.preventDefault();
      e.stopPropagation();
    };

    const handleDrop = () => {
      dragCounterRef.current = 0;
      setIsDragOver(false);
      // drop 由 App.tsx 的 onDragDropEvent 处理
      // 此处仅重置状态
    };

    el.addEventListener("dragenter", handleDragEnter);
    el.addEventListener("dragleave", handleDragLeave);
    el.addEventListener("dragover", handleDragOver);
    el.addEventListener("drop", handleDrop);

    return () => {
      el.removeEventListener("dragenter", handleDragEnter);
      el.removeEventListener("dragleave", handleDragLeave);
      el.removeEventListener("dragover", handleDragOver);
      el.removeEventListener("drop", handleDrop);
      window.__tauterm_dropTarget = undefined;
    };
  }, []);

  // ── 监听父级空白区域右键事件 ──
  // 文件管理器面板内按钮/列表之外的大面积空白区域可能落在
  // RightSidebarPanel 的 DOM 范围内但不在 FileManagerPanel 的 panelRef 内。
  // 父级通过自定义事件通知面板触发空白区域菜单。
  useEffect(() => {
    const handler = (e: Event) => {
      const ce = e as CustomEvent<{ clientX: number; clientY: number }>;
      setCtxX(ce.detail.clientX);
      setCtxY(ce.detail.clientY);
      setCtxTarget(null);
      setCtxVisible(true);
    };
    window.addEventListener("tauterm:filemanager-blank-context", handler);
    return () => window.removeEventListener("tauterm:filemanager-blank-context", handler);
  }, []);

  // ── Context menu state ──────────────────────────────
  const [ctxVisible, setCtxVisible] = useState(false);
  const [ctxX, setCtxX] = useState(0);
  const [ctxY, setCtxY] = useState(0);
  const [ctxTarget, setCtxTarget] = useState<SftpEntry | null>(null);

  const showContextMenu = useCallback(
    (e: React.MouseEvent, entry: SftpEntry | null, _index?: number) => {
      e.preventDefault();
      if (entry !== null) {
        ms.handleRightClick(entry, e.ctrlKey);
      }
      setCtxX(e.clientX);
      setCtxY(e.clientY);
      setCtxTarget(entry);
      setCtxVisible(true);
    },
    [ms],
  );

  const closeContextMenu = useCallback(() => {
    setCtxVisible(false);
  }, []);

  // ── Properties modal state ────────────────────────────
  const [propsVisible, setPropsVisible] = useState(false);
  const [propsTarget, setPropsTarget] = useState<SftpEntry | null>(null);
  const [propsInfo, setPropsInfo] = useState<FileStatInfo | null>(null);
  const [propsLoading, setPropsLoading] = useState(false);

  // ── Preview modal state ───────────────────────────────
  const [previewVisible, setPreviewVisible] = useState(false);
  const [previewFileName, setPreviewFileName] = useState("");
  const [previewContent, setPreviewContent] = useState<string | null>(null);
  const [previewLoading, setPreviewLoading] = useState(false);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [previewFileSize, setPreviewFileSize] = useState(0);

  // ── Entry click / double-click ──────────────────────
  const handleEntryClick = useCallback(
    (entry: SftpEntry, index: number, ctrlKey: boolean, shiftKey: boolean) => {
      ms.handleClick(entry, index, ctrlKey, shiftKey);
    },
    [ms],
  );

  const handleEntryDoubleClick = useCallback(
    (entry: SftpEntry) => {
      if (entry.is_dir) {
        fm.navigateTo(entry.path);
        ms.clearSelection();
      } else {
        fm.downloadFiles([entry]);
      }
    },
    [fm, ms],
  );

  // ── Context menu actions ────────────────────────────
  const handleUpload = useCallback(async () => {
    const selected = await open({ multiple: false });
    if (!selected) return;
    const localPath = typeof selected === "string" ? selected : selected;
    const fileName = localPath.split(/[/\\]/).pop() || "file";
    const remotePath =
      fm.currentPath === "/"
        ? `/${fileName}`
        : `${fm.currentPath}/${fileName}`;
    await fm.uploadFile(localPath, remotePath);
  }, [fm]);

  const handleNewFile = useCallback(() => {
    fm.setPromptMode("newFile");
    fm.setPromptValue("");
  }, [fm]);

  const handleNewFolder = useCallback(() => {
    fm.setPromptMode("newFolder");
    fm.setPromptValue("");
  }, [fm]);

  const handleDownload = useCallback(async () => {
    const targets =
      ms.selectedEntries.length > 0 ? ms.selectedEntries : ctxTarget ? [ctxTarget] : [];
    if (targets.length === 0) return;

    // Single directory → recursive download
    if (targets.length === 1 && targets[0].is_dir) {
      const selected = await open({ directory: true, multiple: false });
      if (!selected) return;
      const localDir = typeof selected === "string" ? selected : selected;
      try {
        await fm.downloadDirectory(targets[0].path, localDir);
      } catch (e) {
        // Error handled by useFileManager
      }
      ms.clearSelection();
      return;
    }

    // Files only
    await fm.downloadFiles(targets.filter(e => !e.is_dir));
    ms.clearSelection();
  }, [fm, ms, ctxTarget]);

  const handleRename = useCallback(() => {
    const target = ms.selectedEntries.length === 1 ? ms.selectedEntries[0] : ctxTarget;
    if (target) {
      fm.setPromptTarget(target);
      fm.setPromptMode("rename");
      fm.setPromptValue(target.name);
    }
  }, [fm, ms, ctxTarget]);

  const handleCopyPath = useCallback(async () => {
    const target = ms.selectedEntries.length === 1 ? ms.selectedEntries[0] : ctxTarget;
    if (target) {
      try {
        await navigator.clipboard.writeText(target.path);
      } catch {
        // Clipboard API may not be available
      }
    }
  }, [ms, ctxTarget]);

  const handleDelete = useCallback(async () => {
    const targets =
      ms.selectedEntries.length > 0 ? ms.selectedEntries : ctxTarget ? [ctxTarget] : [];
    if (targets.length === 0) return;

    const hasDirs = targets.some(e => e.is_dir);
    let msg: string;
    if (targets.length === 1) {
      if (targets[0].is_dir) {
        msg = t("fileManager.deleteDirConfirm", { name: targets[0].name });
      } else {
        msg = t("fileManager.deleteConfirm", { name: targets[0].name });
      }
    } else {
      msg = hasDirs
        ? t("fileManager.confirmBatchDeleteWithDirs", { count: targets.length })
        : t("fileManager.confirmBatchDelete", { count: targets.length });
    }

    if (!window.confirm(msg)) return;
    await fm.deleteEntries(targets);
    ms.clearSelection();
  }, [fm, ms, ctxTarget, t]);

  const handleRefresh = useCallback(async () => {
    await fm.refresh();
    ms.clearSelection();
  }, [fm, ms]);

  // ── 打开目录 ──────────────────────────────────────────
  const handleOpenDir = useCallback(() => {
    const target =
      ms.selectedEntries.length === 1 && ms.selectedEntries[0].is_dir
        ? ms.selectedEntries[0]
        : ctxTarget;
    if (target?.is_dir) {
      fm.navigateTo(target.path);
      ms.clearSelection();
    }
  }, [fm, ms, ctxTarget]);

  // ── 属性弹窗 ──────────────────────────────────────────
  const handleProperties = useCallback(async () => {
    const target = ms.selectedEntries.length === 1 ? ms.selectedEntries[0] : ctxTarget;
    if (!target) return;
    setPropsTarget(target);
    setPropsInfo(null);
    setPropsLoading(true);
    setPropsVisible(true);
    try {
      const info = await invoke<FileStatInfo>("sftp_stat_cmd", {
        sessionId,
        remotePath: target.path,
      });
      setPropsInfo(info);
    } catch (e) {
      // 如果 stat 失败，使用 entry 本身的字段作为回退
      setPropsInfo({
        name: target.name,
        path: target.path,
        isDir: target.is_dir,
        size: target.size,
        modified: target.modified,
        permissions: target.permissions,
      });
    }
    setPropsLoading(false);
  }, [sessionId, ms, ctxTarget]);

  const closeProperties = useCallback(() => {
    setPropsVisible(false);
    setPropsTarget(null);
    setPropsInfo(null);
  }, []);

  // ── 文件预览（使用 sftp_read_head 部分读取，无需临时文件）──
  const handlePreview = useCallback(async () => {
    const target = ms.selectedEntries.length === 1 ? ms.selectedEntries[0] : ctxTarget;
    if (!target || target.is_dir) return;

    const MAX_PREVIEW = 1_048_576; // 1 MB

    setPreviewFileName(target.name);
    setPreviewContent(null);
    setPreviewError(null);
    setPreviewLoading(true);
    setPreviewVisible(true);
    setPreviewFileSize(target.size);

    try {
      const result = await invoke<{ data: number[]; total_size: number }>(
        "sftp_read_head_cmd",
        {
          sessionId,
          remotePath: target.path,
          maxBytes: MAX_PREVIEW,
        },
      );

      const bytes = new Uint8Array(result.data);
      const decoder = new TextDecoder("utf-8", { fatal: false });
      const text = decoder.decode(bytes);

      if (result.total_size > MAX_PREVIEW) {
        const totalStr =
          result.total_size < 1024 * 1024
            ? `${(result.total_size / 1024).toFixed(1)} KB`
            : `${(result.total_size / (1024 * 1024)).toFixed(1)} MB`;
        const shownStr = `${(MAX_PREVIEW / (1024 * 1024)).toFixed(0)} MB`;
        setPreviewContent(
          `--- ${t("fileManager.previewTruncated", { shown: shownStr, total: totalStr })} ---\n\n${text}`,
        );
      } else {
        setPreviewContent(text);
      }
    } catch (e) {
      setPreviewError(String(e));
    }
    setPreviewLoading(false);
  }, [sessionId, ms, ctxTarget, t]);

  const closePreview = useCallback(() => {
    setPreviewVisible(false);
  }, []);

  // ── Inline prompt actions ───────────────────────────
  const handlePromptConfirm = useCallback(
    async (value: string) => {
      if (fm.promptMode === "newFile") {
        await fm.createFile(value);
      } else if (fm.promptMode === "newFolder") {
        await fm.createFolder(value);
      } else if (fm.promptMode === "rename" && fm.promptTarget) {
        await fm.renameEntry(fm.promptTarget, value);
      }
      fm.setPromptMode(null);
      fm.setPromptTarget(null);
    },
    [fm],
  );

  const handlePromptCancel = useCallback(() => {
    fm.setPromptMode(null);
    fm.setPromptTarget(null);
  }, [fm]);

  // ── Keyboard shortcuts ──────────────────────────────
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      // 仅在面板聚焦或其子元素聚焦时响应
      if (!panelRef.current?.contains(document.activeElement)) return;

      if (e.key === "Escape") {
        if (ctxVisible) {
          closeContextMenu();
          return;
        }
        if (fm.promptMode) {
          handlePromptCancel();
          return;
        }
        ms.clearSelection();
        return;
      }

      // 输入框激活时不处理快捷键
      const tag = (e.target as HTMLElement).tagName;
      if (tag === "INPUT" || tag === "TEXTAREA") return;

      if (e.key === "Delete" && ms.selectedEntries.length > 0) {
        e.preventDefault();
        handleDelete();
      } else if (e.key === "F2" && ms.selectedEntries.length === 1) {
        e.preventDefault();
        handleRename();
      } else if (e.key === "c" && (e.ctrlKey || e.metaKey)) {
        if (ms.selectedEntries.length === 1) {
          e.preventDefault();
          handleCopyPath();
        }
      } else if (e.key === "a" && (e.ctrlKey || e.metaKey)) {
        e.preventDefault();
        ms.selectAll(fm.entries);
      }
    };

    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, [
    ctxVisible,
    fm,
    ms,
    closeContextMenu,
    handlePromptCancel,
    handleDelete,
    handleRename,
    handleCopyPath,
  ]);

  // ── Render ───────────────────────────────────────────
  const contextMenuSelectedCount =
    ms.selectedEntries.length > 0
      ? ms.selectedEntries.length
      : ctxTarget
        ? 1
        : 0;

  // ── Build context menu items ─────────────────────────
  const contextMenuItems = useMemo((): ContextMenuItem[] => {
    if (ctxTarget === null) {
      return [
        { id: "upload", label: t("fileManager.upload") },
        { id: "newFile", label: t("fileManager.newFile") },
        { id: "newFolder", label: t("fileManager.newFolder") },
        { id: "sep1", label: "", type: "separator" },
        { id: "refresh", label: t("fileManager.refresh") },
      ];
    }

    if (contextMenuSelectedCount <= 1) {
      if (ctxTarget.is_dir) {
        return [
          { id: "open", label: t("fileManager.open") },
          { id: "sep2", label: "", type: "separator" },
          { id: "rename", label: t("fileManager.rename") },
          { id: "copyPath", label: t("fileManager.copyPath") },
          { id: "sep3", label: "", type: "separator" },
          { id: "properties", label: t("fileManager.properties") },
          { id: "sep4", label: "", type: "separator" },
          { id: "delete", label: t("fileManager.delete"), danger: true },
        ];
      }
      // File
      const items: ContextMenuItem[] = [
        { id: "download", label: t("fileManager.download") },
      ];
      if (isTextFile(ctxTarget.name)) {
        items.push({ id: "preview", label: t("fileManager.preview") });
      }
      items.push(
        { id: "sep5", label: "", type: "separator" },
        { id: "rename", label: t("fileManager.rename") },
        { id: "copyPath", label: t("fileManager.copyPath") },
        { id: "sep6", label: "", type: "separator" },
        { id: "properties", label: t("fileManager.properties") },
        { id: "sep7", label: "", type: "separator" },
        { id: "delete", label: t("fileManager.delete"), danger: true },
      );
      return items;
    }

    // Multi-select
    return [
      {
        id: "download",
        label: t("fileManager.batchDownload", { count: contextMenuSelectedCount }),
      },
      { id: "sep8", label: "", type: "separator" },
      {
        id: "delete",
        label: t("fileManager.batchDelete", { count: contextMenuSelectedCount }),
        danger: true,
      },
    ];
  }, [ctxTarget, contextMenuSelectedCount, t]);

  const handleContextMenuSelect = useCallback(
    (id: string) => {
      switch (id) {
        case "upload": handleUpload(); break;
        case "newFile": handleNewFile(); break;
        case "newFolder": handleNewFolder(); break;
        case "refresh": handleRefresh(); break;
        case "open": handleOpenDir(); break;
        case "download": handleDownload(); break;
        case "preview": handlePreview(); break;
        case "rename": handleRename(); break;
        case "copyPath": handleCopyPath(); break;
        case "properties": handleProperties(); break;
        case "delete": handleDelete(); break;
      }
    },
    [
      handleUpload, handleNewFile, handleNewFolder, handleRefresh,
      handleOpenDir, handleDownload, handlePreview, handleRename,
      handleCopyPath, handleProperties, handleDelete,
    ],
  );

  return (
    <div
      ref={panelRef}
      className={`${styles.panel} ${isDragOver ? styles.dragOver : ""}`}
      tabIndex={-1}
    >
      {/* 面包屑导航 */}
      <BreadcrumbNav
        segments={fm.breadcrumbSegments}
        onNavigate={(path) => {
          fm.navigateTo(path);
          ms.clearSelection();
        }}
      />

      {/* 内联输入提示（新建/重命名） */}
      <InlinePrompt
        visible={fm.promptMode !== null}
        defaultValue={fm.promptMode === "rename" ? fm.promptValue : ""}
        placeholder={
          fm.promptMode === "newFile"
            ? t("fileManager.newFilePrompt")
            : undefined
        }
        onConfirm={handlePromptConfirm}
        onCancel={handlePromptCancel}
      />

      {/* 文件列表 */}
      <FileList
        entries={fm.entries}
        loading={fm.loading}
        error={fm.error}
        selectedPaths={ms.selectedPaths}
        sortField={fm.sortField}
        sortDirection={fm.sortDirection}
        onSortChange={fm.setSortField}
        onEntryClick={handleEntryClick}
        onEntryDoubleClick={handleEntryDoubleClick}
        onContextMenu={showContextMenu}
        onClearError={fm.clearError}
        showParentDir={fm.currentPath !== "/"}
        onGoUp={fm.goUp}
      />

      {/* 传输进度条 */}
      <TransferProgressBar
        visible={progress.visible}
        fileName={progress.fileName}
        direction={progress.direction}
        percent={progress.percent}
        finished={progress.finished}
        speed={progress.speed}
        onClose={() => {
          // 传输进行中：中断后端传输；已完成：仅隐藏进度条
          if (!progress.finished) {
            cancelTransfer();
          } else {
            hideProgress();
          }
        }}
      />

      {/* 右键菜单 */}
      <CommonContextMenu
        state={{ x: ctxX, y: ctxY, visible: ctxVisible, session: null }}
        items={contextMenuItems}
        onSelect={handleContextMenuSelect}
        onClose={closeContextMenu}
        header={
          ctxTarget === null
            ? null
            : contextMenuSelectedCount <= 1
              ? {
                  icon: ctxTarget.is_dir ? "\u{1F4C1}" : "\u{1F4C4}",
                  label: ctxTarget.name,
                }
              : {
                  icon: "📋",
                  label: t("fileManager.selectedCount", { count: contextMenuSelectedCount }),
                }
        }
      />

      {/* 文件属性弹窗 */}
      <FilePropertiesModal
        visible={propsVisible}
        entry={propsTarget}
        statInfo={propsInfo}
        loading={propsLoading}
        onClose={closeProperties}
        sessionId={sessionId}
        onChmodComplete={() => {
          if (propsTarget) {
            setPropsLoading(true);
            invoke<FileStatInfo>("sftp_stat_cmd", {
              sessionId,
              remotePath: propsTarget.path,
            })
              .then(setPropsInfo)
              .catch(() => {})
              .finally(() => setPropsLoading(false));
          }
        }}
      />

      {/* 文本预览弹窗 */}
      <FilePreviewModal
        visible={previewVisible}
        fileName={previewFileName}
        content={previewContent}
        loading={previewLoading}
        error={previewError}
        fileSize={previewFileSize}
        onClose={closePreview}
      />
    </div>
  );
}
