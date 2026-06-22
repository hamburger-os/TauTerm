import { useCallback } from "react";
import { useTranslation } from "react-i18next";
import { useSession } from "../../context/SessionContext";
import { pluginRegistry } from "../../core/plugin-registry";
import type { ToolbarItem } from "../../core/plugin-registry";
import styles from "./Toolbar.module.css";

interface ToolbarProps {
  onAction: (actionId: string) => void;
}

/**
 * 功能工具栏
 *
 * 三区布局：
 *   左侧 — 全局按钮（新建会话、侧栏切换）
 *   中央 — 活跃插件的工具栏项（动态注入）
 *   右侧 — 命令面板、设置
 */
export default function Toolbar({ onAction }: ToolbarProps) {
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

  return (
    <div className={`${styles.toolbar} liquid-glass`}>
      {/* 左侧：Logo + 侧栏图标按钮 + 插件左区 */}
      <div className={styles.leftZone}>
        <span className={styles.logo}>⚡ TauTerm</span>

        <button
          className={styles.toolbarButton}
          onClick={() => handleClick("sidebar")}
          title={t("toolbar.sidebar") + " (Ctrl+B)"}
        >
          <span className={styles.icon}>☰</span>
        </button>

        {/* 插件左区按钮 */}
        {leftItems.map(item => (
          <button
            key={item.id}
            className={styles.toolbarButton}
            onClick={item.onClick}
            title={item.label}
          >
            <span className={styles.icon}>{item.icon}</span>
            <span className={styles.label}>{item.label}</span>
          </button>
        ))}
      </div>

      {/* 中央：插件中区 */}
      <div className={styles.centerZone}>
        {centerItems.map(item => (
          <button
            key={item.id}
            className={styles.toolbarButton}
            onClick={item.onClick}
            title={item.label}
          >
            <span className={styles.icon}>{item.icon}</span>
            <span className={styles.label}>{item.label}</span>
          </button>
        ))}
      </div>

      {/* 右侧：插件右区 + 命令面板图标按钮 */}
      <div className={styles.rightZone}>
        {rightItems.map(item => (
          <button
            key={item.id}
            className={styles.toolbarButton}
            onClick={item.onClick}
            title={item.label}
          >
            <span className={styles.icon}>{item.icon}</span>
            <span className={styles.label}>{item.label}</span>
          </button>
        ))}

        <button
          className={styles.toolbarButton}
          onClick={() => handleClick("commands")}
          title={t("toolbar.commands") + " (Ctrl+Shift+P)"}
        >
          <span className={styles.icon}>⌘</span>
        </button>
      </div>
    </div>
  );
}
