import { useCallback } from "react";
import { useTranslation } from "react-i18next";
import styles from "./Toolbar.module.css";

interface ToolbarButton {
  id: string;
  icon: string;
  labelKey: string;
  shortcut?: string;
}

const TOOLBAR_BUTTONS: ToolbarButton[] = [
  { id: "newSession", icon: "➕", labelKey: "toolbar.newSession", shortcut: "Ctrl+N" },
  { id: "sidebar", icon: "☰", labelKey: "toolbar.sidebar", shortcut: "Ctrl+B" },
  { id: "commands", icon: "⌘", labelKey: "toolbar.commands", shortcut: "Ctrl+P" },
  { id: "settings", icon: "⚙", labelKey: "toolbar.settings" },
];

interface ToolbarProps {
  onAction: (actionId: string) => void;
}

/**
 * 顶部功能工具栏
 *
 * 图标+文字按钮，替代原有的 QuickConnectBar。
 * 提供新建会话、刷新端口、传输面板、侧栏切换、命令面板五个常用操作。
 */
export default function Toolbar({ onAction }: ToolbarProps) {
  const { t } = useTranslation();

  const handleClick = useCallback(
    (id: string) => {
      onAction(id);
    },
    [onAction]
  );

  return (
    <div className={styles.toolbar}>
      <span className={styles.logo}>⚡ TauTerm</span>
      <div className={styles.buttons}>
        {TOOLBAR_BUTTONS.map((btn) => (
          <button
            key={btn.id}
            className={styles.toolbarButton}
            onClick={() => handleClick(btn.id)}
            title={btn.shortcut ? `${t(btn.labelKey)} (${btn.shortcut})` : t(btn.labelKey)}
          >
            <span className={styles.icon}>{btn.icon}</span>
            <span className={styles.label}>{t(btn.labelKey)}</span>
          </button>
        ))}
      </div>
    </div>
  );
}
