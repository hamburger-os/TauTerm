import { useCallback, useMemo, useRef } from "react";
import { useTranslation } from "react-i18next";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useSession } from "../../context/SessionContext";
import { pluginRegistry } from "../../core/plugin-registry";
import type { ToolbarItem } from "../../core/plugin-registry";
import { shortcutRegistry } from "../../shortcuts/registry";
import { ACTION_IDS } from "../../shortcuts/actionIds";
import Icon from "../common/Icon";
import TitleBar, { needsCustomTitleBar } from "./TitleBar";
import styles from "./Toolbar.module.css";

interface ToolbarProps {
  onAction: (actionId: string) => void;
  isMaximized: boolean;
}

/**
 * 功能工具栏
 *
 * 工具栏整体可拖动窗口，交互子元素（按钮、输入框）自动排除：
 *   左侧 — Logo + 全局按钮（新建会话、侧栏切换）
 *   中央 — VSCode 风格搜索/命令触发器 + 插件工具栏项
 *   右侧 — 窗口控制按钮
 */
export default function Toolbar({ onAction, isMaximized }: ToolbarProps) {
  const { t } = useTranslation();
  const { state } = useSession();
  const activeTab = state.tabs.find(t => t.id === state.activeTabId);

  // 获取活跃插件的工具栏项
  const pluginToolbarItems: ToolbarItem[] = activeTab
    ? pluginRegistry.get(activeTab.pluginId)?.toolbarItems ?? []
    : [];

  const leftItems = pluginToolbarItems.filter(i => i.position === "left");
  const centerItems = pluginToolbarItems.filter(i => i.position === "center");
  const rightItems = pluginToolbarItems.filter(i => i.position === "right");

  /** 手动双击检测 ref：记录上次 mousedown 的时间与坐标 */
  const lastMouseDownRef = useRef<{ time: number; x: number; y: number } | null>(null);

  const handleClick = useCallback(
    (id: string) => onAction(id),
    [onAction]
  );

  // 缓存快捷键字符串，避免每次渲染都遍历查找
  // 注意：依赖数组为空，快捷键提示仅在挂载时计算一次。
  // 如未来支持运行时快捷键重绑定，需添加订阅机制以触发重新计算。
  const sidebarShortcut = useMemo(() => {
    return shortcutRegistry.getAll().find(s => s.id === ACTION_IDS.SIDEBAR_TOGGLE)?.keys;
  }, []);
  const rightSidebarShortcut = useMemo(() => {
    return shortcutRegistry.getAll().find(s => s.id === ACTION_IDS.RIGHT_SIDEBAR_TOGGLE)?.keys;
  }, []);

  /** 工具栏非交互区域按下鼠标 → 触发窗口拖动 / 双击最大化 */
  const handleToolbarMouseDown = useCallback((e: React.MouseEvent) => {
    // 仅当点击目标是工具栏容器自身或 spacer（非交互元素）时触发
    const target = e.target as HTMLElement;
    if (target.closest("button, input, a, [role='button']")) return;

    // 手动检测双击：在两次 mousedown 间距 < 300ms 且坐标变化 < 5px 时触发最大化/还原。
    // 必须在 startDragging() 之前检测：startDragging() 会启动系统级窗口拖拽，
    // 吞掉后续 mousedown 事件，导致 DOM dblclick 事件无法可靠触发。
    const now = Date.now();
    const last = lastMouseDownRef.current;
    if (
      needsCustomTitleBar() &&
      last &&
      now - last.time < 300 &&
      Math.abs(e.clientX - last.x) < 5 &&
      Math.abs(e.clientY - last.y) < 5
    ) {
      lastMouseDownRef.current = null;
      getCurrentWindow().toggleMaximize();
      return;
    }

    lastMouseDownRef.current = { time: now, x: e.clientX, y: e.clientY };
    getCurrentWindow().startDragging();
  }, []);

  return (
    <div className={`${styles.toolbar} liquid-glass`} onMouseDown={handleToolbarMouseDown}>
      {/* 左侧：Logo（可拖动）+ 侧栏图标按钮 + 插件左区 */}
      <div className={styles.leftZone}>
        <span className={styles.logo}><Icon name="logo" size="lg" /> TauTerm</span>

        <button
          className={styles.toolbarButton}
          onClick={() => handleClick("sidebar")}
          title={t("toolbar.sidebar") + (sidebarShortcut ? ` (${sidebarShortcut})` : "")}
        >
          <Icon name="menu" size="sm" className={styles.icon} />
        </button>

        {/* 插件左区按钮 */}
        {leftItems.map(item => (
          <button
            key={item.id}
            className={styles.toolbarButton}
            onClick={item.onClick}
            title={item.label}
          >
            <Icon name={item.icon} size="sm" className={styles.icon} />
            <span className={styles.label}>{item.label}</span>
          </button>
        ))}
      </div>

      {/* 弹性占位 — 左侧区域与中央区域之间的空白 */}
      <div className={styles.dragSpacer} />

      {/* 中央：VSCode 风格搜索/命令触发器 + 插件中区 */}
      <div className={styles.centerZone}>
        {/* VSCode 风格命令搜索栏 — 点击打开命令面板 */}
        <div
          className={styles.searchTrigger}
          onClick={() => handleClick("commands")}
          role="button"
          tabIndex={0}
          onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") handleClick("commands"); }}
          aria-label={t("toolbar.commands")}
          title={t("toolbar.commands") + " (Ctrl+Shift+P)"}
        >
          <Icon name="search" size="sm" className={styles.searchIcon} />
          <span className={styles.searchPlaceholder}>
            {t("toolbar.searchPlaceholder") || "Search files or run commands..."}
          </span>
        </div>

        <button
          className={styles.toolbarButton}
          onClick={() => handleClick("rightSidebar")}
          title={t("toolbar.rightSidebar") + (rightSidebarShortcut ? ` (${rightSidebarShortcut})` : "")}
        >
          <Icon name="panel-right" size="sm" className={styles.icon} />
        </button>

        {centerItems.map(item => (
          <button
            key={item.id}
            className={styles.toolbarButton}
            onClick={item.onClick}
            title={item.label}
          >
            <Icon name={item.icon} size="sm" className={styles.icon} />
            <span className={styles.label}>{item.label}</span>
          </button>
        ))}
      </div>

      {/* 弹性占位 — 中央区域与右侧区域之间的空白 */}
      <div className={styles.dragSpacer} />

      {/* 右侧：插件右区 + 右侧栏切换按钮 + 命令面板图标按钮 + 窗口控制 — 无拖动区域，确保按钮点击生效 */}
      <div className={styles.rightZone}>
        {rightItems.map(item => (
          <button
            key={item.id}
            className={styles.toolbarButton}
            onClick={item.onClick}
            title={item.label}
          >
            <Icon name={item.icon} size="sm" className={styles.icon} />
            <span className={styles.label}>{item.label}</span>
          </button>
        ))}

        <TitleBar isMaximized={isMaximized} />
      </div>
    </div>
  );
}
