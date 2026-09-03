import { useRef, useState, useEffect } from "react";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useSession } from "../../context/SessionContext";
import styles from "./RightSidebar.module.css";

interface RightSidebarProps {
  children: ReactNode;
}

/**
 * 右侧栏容器组件
 *
 * 使用 liquid-glass 样式，与左侧 SessionSidebar 视觉一致。
 * 内部内容支持 `overflow-y: auto` 滚动，当面板总高度超出可视区域时自动出现滚动条。
 * 溢出时通过 ResizeObserver 检测并在底部显示淡出渐变提示。
 * 宽度由父容器统控制（motion.div 动画宽度）。
 *
 * Split View 选中空 Pane 时 activeTabId 为空。此时 App 仍可能保留右侧栏外壳，
 * 因此通过 .sidebarEmpty 标记让 CSS 同时折叠外层 motion 容器与 resize handle。
 */
export default function RightSidebar({ children }: RightSidebarProps) {
  const { t } = useTranslation();
  const { state: sessionState } = useSession();
  const scrollRef = useRef<HTMLDivElement>(null);
  const [isScrollable, setIsScrollable] = useState(false);
  const hasActiveSession = Boolean(
    sessionState.activeTabId
    && sessionState.tabs.some(tab => tab.id === sessionState.activeTabId)
  );

  useEffect(() => {
    const el = scrollRef.current;
    if (!el || !hasActiveSession) {
      setIsScrollable(false);
      return;
    }
    const check = () => setIsScrollable(el.scrollHeight > el.clientHeight);
    const observer = new ResizeObserver(check);
    observer.observe(el);
    check();
    return () => observer.disconnect();
  }, [children, hasActiveSession]);

  return (
    <aside
      className={`${styles.sidebar} ${hasActiveSession ? "" : styles.sidebarEmpty} liquid-glass`}
      aria-label={t("rightSidebar.ariaLabel")}
      aria-hidden={!hasActiveSession}
    >
      <div
        ref={scrollRef}
        className={`${styles.scrollArea} ${isScrollable ? styles.scrollable : ""}`}
      >
        {children}
      </div>
    </aside>
  );
}
