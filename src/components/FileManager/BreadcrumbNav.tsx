/**
 * 面包屑导航组件
 *
 * 可点击的路径段，用于文件管理器目录导航。
 */
import { useTranslation } from "react-i18next";
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
  const { t } = useTranslation();

  return (
    <nav className={styles.breadcrumb} aria-label={t("fileManager.path")}>
      {segments.map((seg, i) => {
        const current = i === segments.length - 1;
        return (
          <span key={seg.path} className={styles.segmentWrapper}>
            {i > 0 && <span className={styles.separator}>/</span>}
            {current ? (
              <span className={styles.current} aria-current="page">
                {seg.name}
              </span>
            ) : (
              <button
                type="button"
                className={styles.segment}
                onClick={() => onNavigate(seg.path)}
              >
                {seg.name}
              </button>
            )}
          </span>
        );
      })}
    </nav>
  );
}
