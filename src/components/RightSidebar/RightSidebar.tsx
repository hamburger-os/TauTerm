import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useSession } from "../../context/SessionContext";
import styles from "./RightSidebar.module.css";

interface RightSidebarProps {
  children: ReactNode;
}

/**
 * 右侧栏只负责布局与会话可见性。
 *
 * 结构材质由 App.tsx 外层统一的 .liquid-glass-panel 提供，与左侧栏完全共用一层，
 * 不再为装饰性底部渐隐维护 ResizeObserver，避免重复 glass 与无意义的持续尺寸监听。
 */
export default function RightSidebar({ children }: RightSidebarProps) {
  const { t } = useTranslation();
  const { state: sessionState } = useSession();
  const hasActiveSession = Boolean(
    sessionState.activeTabId
    && sessionState.tabs.some(tab => tab.id === sessionState.activeTabId)
  );

  return (
    <aside
      className={`${styles.sidebar} ${hasActiveSession ? "" : styles.sidebarEmpty}`}
      aria-label={t("rightSidebar.ariaLabel")}
      aria-hidden={!hasActiveSession}
    >
      <div className={styles.scrollArea}>
        {children}
      </div>
    </aside>
  );
}
