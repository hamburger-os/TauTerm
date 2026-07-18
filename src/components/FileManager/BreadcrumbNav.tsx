/**
 * 面包屑导航组件
 *
 * 可点击的路径段，用于文件管理器目录导航。
 */
import styles from "./BreadcrumbNav.module.css";

interface BreadcrumbSegments {
  name: string;
  path: string;
}

interface BreadcrumbNavProps {
  segments: BreadcrumbSegments[];
  onNavigate: (path: string) => void;
}

export default function BreadcrumbNav({ segments, onNavigate }: BreadcrumbNavProps) {
  return (
    <div className={styles.breadcrumb}>
      {segments.map((seg, i) => (
        <span key={seg.path} className={styles.segmentWrapper}>
          {i > 0 && <span className={styles.separator}>/</span>}
          <span
            className={`${styles.segment} ${i === segments.length - 1 ? styles.current : ""}`}
            onClick={() => onNavigate(seg.path)}
            role="button"
            tabIndex={0}
            onKeyDown={(e) => {
              if (e.key === "Enter" || e.key === " ") onNavigate(seg.path);
            }}
          >
            {seg.name}
          </span>
        </span>
      ))}
    </div>
  );
}
