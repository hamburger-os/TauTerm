import { computePaneRects, type LayoutNode, type PaneId } from "../../core/split-layout";
import styles from "./PaneMiniMap.module.css";

interface PaneMiniMapProps {
  layout: LayoutNode;
  /** 单个 Session 通常只在一个 Pane；保留 paneIds 以支持父会话未来汇总多个子 Pane。 */
  paneId?: PaneId;
  paneIds?: PaneId | readonly PaneId[];
  selected?: boolean;
  title?: string;
}

export default function PaneMiniMap({ layout, paneId, paneIds, selected = false, title }: PaneMiniMapProps) {
  const rects = computePaneRects(layout);
  const source = paneIds ?? paneId;
  if (!source) return null;
  const highlighted = new Set(Array.isArray(source) ? source : [source]);
  return (
    <span
      className={`${styles.wrapper} ${selected ? styles.selected : ""}`}
      title={title}
      aria-label={title}
    >
      <svg viewBox="0 0 100 100" className={styles.svg} aria-hidden="true">
        <rect x="2" y="2" width="96" height="96" rx="10" className={styles.frame} />
        {Object.entries(rects).map(([id, rect]) => (
          <rect
            key={id}
            x={rect.left * 100 + 4}
            y={rect.top * 100 + 4}
            width={Math.max(0, rect.width * 100 - 8)}
            height={Math.max(0, rect.height * 100 - 8)}
            rx="5"
            className={highlighted.has(id) ? styles.currentPane : styles.otherPane}
          />
        ))}
      </svg>
    </span>
  );
}
