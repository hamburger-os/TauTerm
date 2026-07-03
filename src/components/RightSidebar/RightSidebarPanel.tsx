import { useState, useCallback, useRef, useEffect } from "react";
import { useTranslation } from "react-i18next";
import type { ReactNode } from "react";
import Icon from "../common/Icon";
import styles from "./RightSidebarPanel.module.css";

export interface RightSidebarPanelProps {
  title: string;
  defaultExpanded?: boolean;
  children: ReactNode;
}

/**
 * 可折叠面板通用组件
 *
 * 用于右侧栏中各工具框体。标题栏始终可见，点击可折叠/展开内容区。
 * 折叠动画通过 CSS max-height 过渡实现。
 */
export default function RightSidebarPanel({
  title,
  defaultExpanded = true,
  children,
}: RightSidebarPanelProps) {
  const { t } = useTranslation();
  const [expanded, setExpanded] = useState(defaultExpanded);
  const contentRef = useRef<HTMLDivElement>(null);
  const [contentHeight, setContentHeight] = useState(0);

  const toggle = useCallback(() => {
    setExpanded((prev) => !prev);
  }, []);

  // 挂载时同步测量内容高度，避免首次展开时的 max-height 跳变
  useEffect(() => {
    const el = contentRef.current;
    if (el && expanded) {
      setContentHeight(el.scrollHeight);
    }
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // 内容变化时动态更新高度（如用户输入触发结果显示）
  useEffect(() => {
    const el = contentRef.current;
    if (!el || !expanded) return;

    const observer = new ResizeObserver((entries) => {
      for (const entry of entries) {
        const height = entry.target.scrollHeight;
        if (height > 0) {
          setContentHeight(height);
        }
      }
    });

    observer.observe(el);
    return () => observer.disconnect();
  }, [expanded]);

  return (
    <div className={`${styles.panel} ${expanded ? styles.expanded : ""}`}>
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
        className={styles.body}
        style={{
          maxHeight: expanded ? (contentHeight > 0 ? contentHeight + "px" : "2000px") : 0,
          opacity: expanded ? 1 : 0,
        }}
      >
        <div ref={contentRef}>{children}</div>
      </div>
    </div>
  );
}
