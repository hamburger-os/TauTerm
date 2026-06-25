import { useCallback } from "react";
import { useTranslation } from "react-i18next";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useSession } from "../../context/SessionContext";
import { pluginRegistry } from "../../core/plugin-registry";
import type { ToolbarItem } from "../../core/plugin-registry";
import Icon from "../common/Icon";
import TitleBar from "./TitleBar";
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

  const handleClick = useCallback(
    (id: string) => onAction(id),
    [onAction]
  );

  /** 工具栏非交互区域按下鼠标 → 触发窗口拖动 */
  const handleToolbarMouseDown = useCallback((e: React.MouseEvent) => {
    // 仅当点击目标是工具栏容器自身或 spacer（非交互元素）时触发拖动
    const target = e.target as HTMLElement;
    if (target.closest("button, input, a, [role='button']")) return;
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
          title={t("toolbar.sidebar") + " (Ctrl+B)"}
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

      {/* 右侧：插件右区 + 命令面板图标按钮 + 窗口控制 — 无拖动区域，确保按钮点击生效 */}
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
