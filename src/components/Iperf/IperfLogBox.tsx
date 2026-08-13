/**
 * iperf 实时流日志（iperf 风格区间行，mono 字体自动滚动）
 */
import { useEffect, useRef } from "react";
import { useTranslation } from "react-i18next";
import styles from "./IperfSessionView.module.css";

interface Props {
  lines: string[];
}

export default function IperfLogBox({ lines }: Props) {
  const { t } = useTranslation();
  const boxRef = useRef<HTMLDivElement>(null);

  // 新行到达时自动滚动到底部
  useEffect(() => {
    const el = boxRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [lines.length]);

  return (
    <div className={styles.logSection}>
      <h4>{t("iperf.resultLog")}</h4>
      <div ref={boxRef} className={styles.logBox}>
        {lines.length === 0 ? (
          <span className={styles.noData}>{t("iperf.noData")}</span>
        ) : (
          lines.map((line, i) => (
            <div key={i} className={styles.logLine}>{line}</div>
          ))
        )}
      </div>
    </div>
  );
}
