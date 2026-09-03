import { computePaneRects, type LayoutNode, type PaneId } from "../../core/split-layout";
import styles from "./PaneMiniMap.module.css";

interface PaneMiniMapProps {
  layout: LayoutNode;
  paneId: PaneId;
  selected?: boolean;
  title?: string;
}

export default function PaneMiniMap({ layout, paneId, selected = false, title }: PaneMiniMapProps) {
  const rects = computePaneRects(layout);
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
            className={id === paneId ? styles.currentPane : styles.otherPane}
          />
        ))}
      </svg>
    </span>
  );
}
