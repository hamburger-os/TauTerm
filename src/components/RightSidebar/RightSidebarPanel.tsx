import { useState, useCallback } from "react";
import { useTranslation } from "react-i18next";
import type { ReactNode } from "react";
import Icon from "../common/Icon";
import styles from "./RightSidebarPanel.module.css";

export interface RightSidebarPanelProps {
  title: string;
  defaultExpanded?: boolean;
  children: ReactNode;
  /** 根元素右键事件（用于文件管理器空白区域菜单等） */
  onContextMenu?: (e: React.MouseEvent) => void;
}

/**
 * 可折叠面板通用组件
 *
 * 用于右侧栏中各工具框体。标题栏始终可见，点击可折叠/展开内容区。
 * 折叠动画由 CSS grid track 过渡完成，不需要持续测量内容高度。
 */
export default function RightSidebarPanel({
  title,
  defaultExpanded = true,
  children,
  onContextMenu,
}: RightSidebarPanelProps) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(defaultExpanded);
  const toggle = useCallback(() => {
    setExpanded((prev) => !prev);
  }, []);

  return (
    <div
      className={`${styles.panel} ${expanded ? styles.expanded : ""}`}
      onContextMenu={onContextMenu}
    >
      <button
        className={styles.header}
        onClick={toggle}
        type="button"
        aria-expanded={expanded}
        title={expanded ? t("rightSidebar.collapse") : t("rightSidebar.expand")}
      >
        <span
          className={`${styles.chevron} ${expanded ? styles.chevronOpen : ""}`}
        >
          <Icon name="chevron-down" size="xs" />
        </span>
        <span className={styles.title}>{title}</span>
      </button>
      <div
        className={`${styles.body} ${expanded ? styles.bodyExpanded : ""}`}
        aria-hidden={!expanded}
      >
        <div className={styles.bodyInner}>{children}</div>
      </div>
    </div>
  );
}
