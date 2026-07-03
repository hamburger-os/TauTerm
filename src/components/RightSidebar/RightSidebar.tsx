import { useRef, useState, useEffect } from "react";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import styles from "./RightSidebar.module.css";

interface RightSidebarProps {
  children: ReactNode;
  width: number;
}

/**
 * 右侧栏容器组件
 *
 * 使用 liquid-glass 样式，与左侧 SessionSidebar 视觉一致。
 * 内部内容支持 `overflow-y: auto` 滚动，当面板总高度超出可视区域时自动出现滚动条。
 * 溢出时通过 ResizeObserver 检测并在底部显示淡出渐变提示。
 * 宽度由父组件通过 ResizeHandle 拖拽控制。
 */
export default function RightSidebar({ children, width }: RightSidebarProps) {
  const { t } = useTranslation();
  const scrollRef = useRef<HTMLDivElement>(null);
  const [isScrollable, setIsScrollable] = useState(false);

  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const check = () => setIsScrollable(el.scrollHeight > el.clientHeight);
    const observer = new ResizeObserver(check);
    observer.observe(el);
    check();
    return () => observer.disconnect();
  }, [children]);

  return (
    <aside
      className={`${styles.sidebar} liquid-glass`}
      style={{ width }}
      aria-label={t("rightSidebar.ariaLabel")}
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
